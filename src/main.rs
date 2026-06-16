use eframe::egui;
use image::ImageReader;
use ocrs::{ImagePixels, ImageSource};
use ocrs::{OcrEngine, OcrEngineParams};
use rten::Model;
use std::error::Error;
use std::io::Cursor;
use std::sync::mpsc;
use tesseract::Tesseract;

mod data_holders;

use data_holders::AppData;
use data_holders::CaptchaIdResponse;

struct CaptchaApp {
    captcha_text: String,
    threads: String,
    proxies: String,
    logs: String,
    texture: Option<egui::TextureHandle>,
    tx_log: mpsc::Sender<String>,
    rx_log: mpsc::Receiver<String>,
    tx_captcha: mpsc::Sender<Vec<u8>>,
    rx_captcha: mpsc::Receiver<Vec<u8>>,
    engine: OcrEngine,
}

impl CaptchaApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        app_data: AppData,
        tx_log: mpsc::Sender<String>,
        rx_log: mpsc::Receiver<String>,
        tx_captcha: mpsc::Sender<Vec<u8>>,
        rx_captcha: mpsc::Receiver<Vec<u8>>,
        engine: OcrEngine,
    ) -> Self {
        Self {
            captcha_text: String::new(),
            threads: app_data.threads.join("\n"),
            proxies: app_data.proxies.join("\n"),
            logs: String::new(),
            texture: None,
            tx_log: tx_log,
            rx_log: rx_log,
            tx_captcha: tx_captcha,
            rx_captcha: rx_captcha,
            engine: engine,
        }
    }

    fn save_data(&self) {
        let data = AppData {
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

    fn load_texture_from_bytes(&mut self, ctx: &egui::Context, bytes: Vec<u8>) {
        let image = image::load_from_memory(&bytes).unwrap().to_rgba8();

        let size = [image.width() as usize, image.height() as usize];

        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());

        self.texture = Some(ctx.load_texture("captcha", color_image, Default::default()));
    }

    fn start(&mut self) {
        let tx_log_clone = self.tx_log.clone();
        let tx_captcha_clone = self.tx_captcha.clone();

        tokio::spawn(async move {
            tx_log_clone.send("Receiving captcha...\n".to_string());
            let result = load_captcha().await;

            if result.is_ok() {
                tx_log_clone.send("Captcha received!\n".to_string());
                let bytes_clone = result.unwrap().clone();

                tx_captcha_clone.send(bytes_clone.clone()).unwrap();

                tx_log_clone.send("OCRing...\n".to_string());
                let mut tes = Tesseract::new(None, Some("rus")).expect("Failed to init Tesseract");
                tes = tes.set_image_from_mem(&bytes_clone).unwrap();
                tes.set_page_seg_mode(tesseract::PageSegMode::PsmSingleChar);

                let text = tes.get_text().unwrap();
                tx_log_clone.send(format!("Captcha: {}\n", text.trim()));
            } else {
                tx_log_clone.send("Failed `to load captcha.\n".to_string());
            }
        });
    }
}

async fn load_captcha() -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::new();

    let response: CaptchaIdResponse = client
        .get("/cgi/captcha?task=get_id&json=1")
        .send()
        .await?
        .json()
        .await?;

    let image_url = format!("/cgi/captcha?task=get_image&id={}", response.id);

    let image_bytes = client.get(image_url).send().await?.bytes().await?;

    Ok(image_bytes.to_vec())
}

impl eframe::App for CaptchaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            ui.vertical_centered(|ui| ui.label(egui::RichText::new("Autobump").size(25.0)));

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
                    let b_clone = bytes.clone();

                    // Load texture from bytes
                    self.load_texture_from_bytes(ctx, bytes);
                }
                if let Some(texture) = &self.texture {
                    ui.image(texture);
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
    let det_model = Model::load_file("text-detection.rten")?;
    let rec_model = Model::load_file("text-recognition.rten")?;

    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(det_model),
        recognition_model: Some(rec_model),
        ..Default::default()
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 680.0])
            .with_resizable(false),
        ..Default::default()
    };

    let (tx_log, rx_log) = mpsc::channel::<String>();
    let (tx_captcha, rx_captcha) = mpsc::channel::<Vec<u8>>();

    let app_data = load_data();

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
                engine.unwrap(),
            )))
        }),
    )?;

    Ok(())
}
