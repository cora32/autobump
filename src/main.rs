use eframe::egui;
use image::ImageReader;
use image::{DynamicImage, GrayImage, Luma, imageops};
use imageproc::contrast::adaptive_threshold;
use imageproc::distance_transform::Norm;
use imageproc::morphology::{dilate, erode};
use ocrs::{ImagePixels, ImageSource};
use ocrs::{OcrEngine, OcrEngineParams};
use rten::Model;
use std::error::Error;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::mpsc;
use tesseract::Tesseract;
use tokio::sync::Mutex;

mod data_holders;

use data_holders::AppData;
use data_holders::CaptchaIdResponse;

struct CaptchaApp {
    host: String,
    captcha_text: String,
    threads: String,
    proxies: String,
    logs: String,
    texture_original: Option<egui::TextureHandle>,
    texture_denoised: Option<egui::TextureHandle>,
    tx_log: mpsc::Sender<String>,
    rx_log: mpsc::Receiver<String>,
    tx_captcha: mpsc::Sender<Vec<u8>>,
    rx_captcha: mpsc::Receiver<Vec<u8>>,
    tx_captcha_denoised: mpsc::Sender<Vec<u8>>,
    rx_captcha_denoised: mpsc::Receiver<Vec<u8>>,
    tes_arc: Arc<tokio::sync::Mutex<Option<Tesseract>>>,
}

impl CaptchaApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        app_data: AppData,
        tx_log: mpsc::Sender<String>,
        rx_log: mpsc::Receiver<String>,
        tx_captcha: mpsc::Sender<Vec<u8>>,
        rx_captcha: mpsc::Receiver<Vec<u8>>,
        tx_captcha_denoised: mpsc::Sender<Vec<u8>>,
        rx_captcha_denoised: mpsc::Receiver<Vec<u8>>,
        tes_arc: Arc<tokio::sync::Mutex<Option<Tesseract>>>,
    ) -> Self {
        Self {
            host: app_data.host,
            captcha_text: String::new(),
            threads: app_data.threads.join("\n"),
            proxies: app_data.proxies.join("\n"),
            logs: String::new(),
            texture_original: None,
            texture_denoised: None,
            tx_log: tx_log,
            rx_log: rx_log,
            tx_captcha: tx_captcha,
            rx_captcha: rx_captcha,
            tx_captcha_denoised: tx_captcha_denoised,
            rx_captcha_denoised: rx_captcha_denoised,
            tes_arc: tes_arc,
        }
    }

    fn save_data(&self) {
        let data = AppData {
            host: self.host.clone(),
            threads: self.threads.lines().map(|s| s.to_string()).collect(),
            proxies: self.proxies.lines().map(|s| s.to_string()).collect(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&data) {
            std::fs::write("data.json", json).expect("Failed to write data");
        }
    }

    fn check_proxies(&mut self, proxies: Vec<String>) -> Vec<String> {
        let mut valid_proxies = Vec::new();

        for proxy in proxies {
            if self.validate_proxy(&proxy) {
                valid_proxies.push(proxy);
            } else {
                self.logs.push_str(&format!("Invalid proxy: {}\n", proxy));
            }
        }

        valid_proxies
    }

    fn validate_proxy(&self, proxy: &str) -> bool {
        // Simple validation for proxy format (e.g., "http://ip:port")
        let parts: Vec<&str> = proxy.split("://").collect();
        if parts.len() != 2 {
            return false;
        }

        let address_parts: Vec<&str> = parts[1].split(':').collect();
        if address_parts.len() != 2 {
            return false;
        }

        true
    }

    fn load_texture_from_bytes(
        ctx: &egui::Context,
        bytes: Vec<u8>,
        texture: &mut Option<egui::TextureHandle>,
    ) {
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

        let size = [image.width() as usize, image.height() as usize];

        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());

        *texture = Some(ctx.load_texture("captcha_original", color_image, Default::default()));
    }

    fn clean_captcha(raw_bytes: &[u8]) -> Vec<u8> {
        // 1. Load the image and convert to Grayscale
        let img = image::load_from_memory(raw_bytes)
            .expect("Failed to load image")
            .to_luma8();

        // 2. Scale up: Captchas are usually too small for Tesseract.
        // Making it 2x or 3x larger helps OCR accuracy significantly.
        let (w, h) = img.dimensions();
        let img = imageops::resize(&img, w * 2, h * 2, imageops::FilterType::Lanczos3);

        // 3. Adaptive Thresholding: This is better than a fixed threshold for removing
        // background lines that have different color intensities.
        // '8' is the block radius; adjust this based on line thickness.
        let binarized = adaptive_threshold(&img, 6, 50);

        // 4. Denoise: Use Erosion then Dilation (Opening) to remove small dots/thin lines
        let denoised = erode(&binarized, Norm::LInf, 3);
        let final_img = dilate(&denoised, Norm::LInf, 1);

        // 5. Convert back to bytes for Tesseract
        let mut buffer = std::io::Cursor::new(Vec::new());
        final_img
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("Failed to write to buffer");

        buffer.into_inner()
    }

    fn start(&mut self) {
        let tx_log = self.tx_log.clone();
        let tx_captcha = self.tx_captcha.clone();
        let tx_captcha_denoised = self.tx_captcha_denoised.clone();
        let tes = self.tes_arc.clone();
        let host = self.host.clone();

        tokio::spawn(async move {
            tx_log.send("Receiving captcha...\n".to_string());
            let result = load_captcha(host).await;

            if result.is_ok() {
                tx_log.send("Captcha received!\n".to_string());
                let bytes_clone = result.unwrap().clone();

                // Show captcha in UI
                tx_captcha.send(bytes_clone.clone()).unwrap();

                // =============== DENOISER ================
                tx_log.send("Denoising...\n".to_string());

                let clean_bytes = Self::clean_captcha(&bytes_clone);
                tx_captcha_denoised.send(clean_bytes.clone()).unwrap();

                // =============== /DENOISER ================

                // ================ TESSERACT OCR ================
                tx_log.send("OCRing...\n".to_string());
                // Recognize text
                let mut guard = tes.lock().await;
                let c_tes = guard.take().unwrap();

                let mut tesseract = c_tes.set_image_from_mem(&clean_bytes).unwrap();

                let captcha_text = tesseract.get_text().unwrap();

                *guard = Some(tesseract);
                // ================ /TESSERACT OCR ================

                tx_log.send(format!("Captcha: {}\n", captcha_text.trim()));
            } else {
                tx_log.send("Failed `to load captcha.\n".to_string());
            }
        });
    }
}

async fn load_captcha(host: String) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::new();

    let response: CaptchaIdResponse = client
        .get(&format!("{}/cgi/captcha?task=get_id&json=1", host))
        .send()
        .await?
        .json()
        .await?;

    let image_url = format!("{}/cgi/captcha?task=get_image&id={}", host, response.id);

    let image_bytes = client.get(image_url).send().await?.bytes().await?;

    Ok(image_bytes.to_vec())
}

// ================ UI =================

impl eframe::App for CaptchaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            ui.vertical_centered(|ui| ui.label(egui::RichText::new("Autobump").size(25.0)));

            ui.add_space(10.0);

            ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(680.0));

            ui.add_space(10.0);

            // Threads
            ui.label("Threads to bump:");
            ui.add(
                egui::TextEdit::multiline(&mut self.threads)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(10.0);

            // Proxies
            ui.label("Proxies:");
            ui.add(
                egui::TextEdit::multiline(&mut self.proxies)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(10.0);

            //Bump button
            let btn = egui::Button::new(egui::RichText::new("Bump!").size(20.0));
            ui.vertical_centered(|ui| {
                if ui.add(btn).clicked() {
                    self.save_data();

                    self.start()
                }

                ui.add_space(10.0);
                // Captcha image
                if let Ok(bytes) = self.rx_captcha.try_recv() {
                    // Load texture from bytes
                    Self::load_texture_from_bytes(ctx, bytes, &mut self.texture_original);
                }
                if let Some(texture) = &self.texture_original {
                    ui.image(texture);
                }
                // Denoised Captcha image
                if let Ok(bytes) = self.rx_captcha_denoised.try_recv() {
                    // Load texture from bytes
                    Self::load_texture_from_bytes(ctx, bytes, &mut self.texture_denoised);
                }
                if let Some(texture) = &self.texture_denoised {
                    ui.add(egui::Image::new(texture).shrink_to_fit());
                }
            });

            ui.add_space(10.0);

            // Logs
            ui.label("Logs:");
            ui.add(
                egui::TextEdit::multiline(&mut self.logs)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            );

            while let Ok(message) = self.rx_log.try_recv() {
                self.logs.push_str(message.as_str());
            }
        });
    }
}

fn load_data() -> AppData {
    match std::fs::read_to_string("data.json") {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AppData::default(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let det_model = Model::load_file("text-detection.rten")?;
    // let rec_model = Model::load_file("text-recognition.rten")?;

    // let engine = OcrEngine::new(OcrEngineParams {
    //     detection_model: Some(det_model),
    //     recognition_model: Some(rec_model),
    //     ..Default::default()
    // });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 780.0])
            .with_resizable(false),
        ..Default::default()
    };

    let mut tes = Tesseract::new(None, Some("rus"))
        .expect("Failed to init Tesseract")
        .set_variable(
            "tessedit_char_whitelist",
            "абвгдеёжзийклмнопрстуфхцчшщъыьэюя",
        )
        .unwrap()
        .set_variable("load_system_dawg", "0")
        .unwrap()
        .set_variable("load_freq_dawg", "0")
        .unwrap()
        .set_variable("tessedit_ocr_engine_mode", "1")
        .unwrap();

    tes.set_page_seg_mode(tesseract::PageSegMode::PsmSingleWord);
    let tes_arc = Arc::new(Mutex::new(Some(tes)));

    let (tx_log, rx_log) = mpsc::channel::<String>();
    let (tx_captcha, rx_captcha) = mpsc::channel::<Vec<u8>>();
    let (tx_captcha_denoised, rx_captcha_denoised) = mpsc::channel::<Vec<u8>>();

    let app_data = load_data();

    println!("app_data: {}", app_data.host);

    eframe::run_native(
        "Autobump :3",
        options,
        Box::new(|cc| {
            Ok(Box::new(CaptchaApp::new(
                cc,
                app_data,
                tx_log,
                rx_log,
                tx_captcha,
                rx_captcha,
                tx_captcha_denoised,
                rx_captcha_denoised,
                tes_arc,
            )))
        }),
    )?;

    Ok(())
}
