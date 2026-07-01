use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use eframe::glow::Texture;
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
use std::sync::mpsc::{self};
use tesseract::Tesseract;
use tokio::sync::Mutex;
mod app_state;
mod data_holders;
mod utils;

use data_holders::AppData;
use data_holders::CaptchaIdResponse;

use crate::app_state::AppState;
use crate::utils::load_texture_from_bytes;

struct BumpApp {
    state: AppState,
    new_thread_input: String,
    board_input: String,
    captcha_input: String,
    active_index: Option<usize>,
    texture: Option<egui::TextureHandle>,
}

impl BumpApp {
    fn new() -> Self {
        let state = AppState::new();
        let host = state.app_data.host.clone();

        Self {
            state,
            new_thread_input: String::new(),
            captcha_input: String::new(),
            board_input: host,
            active_index: None,
            texture: None,
        }
    }

    fn link(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        active_index: &Option<usize>,
        current_index: usize,
        thread: &str,
        captcha_input: &mut String,
        rx_captcha: Receiver<Vec<u8>>,
        texture: &mut Option<egui::TextureHandle>,
        on_click: impl FnOnce(),
        on_load_captcha: impl FnOnce(),
        on_enter: &mut impl FnMut(String),
    ) {
        ui.allocate_ui(egui::vec2(ui.available_width(), 50.0), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(thread);

                //Captcha and button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Captcha image
                    if let Ok(bytes) = rx_captcha.try_recv() {
                        // Load texture from bytes
                        load_texture_from_bytes(ctx, bytes, texture);
                    }

                    let size = egui::vec2(100.0, 40.0);
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

                    ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
                    let active_index_value = *active_index;
                    if let Some(texture) = &texture
                        && active_index_value != None
                        && active_index_value.unwrap() == current_index
                    {
                        //Image
                        ui.put(rect, egui::Image::new(texture).fit_to_exact_size(size));

                        //Input
                        let response = ui.add(
                            egui::TextEdit::singleline(captcha_input)
                                .desired_width(60.0)
                                .hint_text("Enter captcha..."),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            on_enter(captcha_input.clone());
                        }
                    } else {
                        ui.put(
                            rect,
                            egui::Label::new("No image").sense(egui::Sense::hover()),
                        );
                    }

                    //Btn Del
                    if ui.button("❌").clicked() {
                        on_click();
                    }
                    //Btn bump
                    if ui.button("Bump!").clicked() {
                        on_load_captcha();
                    }
                });
            });
        });
    }
}

impl eframe::App for BumpApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut board_input = None;
        let mut thread_to_add = None;
        let mut index_to_delete = None;
        let mut load_captcha_id = None;
        let mut captcha_text = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            ui.vertical_centered(|ui| ui.label(egui::RichText::new("Autobump :3").size(16.0)));

            ui.add_space(10.0);

            // Drawing links
            for (i, thread) in self.state.app_data.threads.iter().enumerate() {
                let bg_color = if i % 2 == 0 {
                    egui::Color32::from_rgb(19, 19, 19) // Darker
                } else {
                    egui::Color32::from_rgb(16, 16, 16) // Slightly lighter
                };
                let mut on_enter_closure = |text: String| {
                    captcha_text = Some(text);
                };
                egui::Frame::NONE
                    .fill(bg_color) // Dark gray background
                    .inner_margin(0.0) // Padding inside the frame
                    .corner_radius(1.0) // Rounded corners
                    .show(ui, |ui| {
                        Self::link(
                            ui,
                            ctx,
                            &self.active_index,
                            i,
                            thread,
                            &mut self.captcha_input,
                            self.state.rx_captcha.clone(),
                            &mut self.texture,
                            || {
                                index_to_delete = Some(i);
                            },
                            || {
                                load_captcha_id = Some(i);
                            },
                            &mut on_enter_closure,
                        );
                    });
                ui.separator();
            }

            // Add new thread btn
            ui.horizontal(|ui| {
                ui.set_min_width(ui.available_width());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button_res = ui.button("Add");

                    let response = ui.add_sized(
                        ui.available_size(),
                        egui::TextEdit::singleline(&mut self.new_thread_input)
                            .hint_text("Enter thread link..."),
                    );

                    if button_res.clicked()
                        || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        thread_to_add = Some(self.new_thread_input.clone());
                        self.new_thread_input.clear();
                    }
                });
            });

            // Set target btn
            ui.horizontal(|ui| {
                ui.set_min_width(ui.available_width());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let button_res = ui.button("Set");

                    let response = ui.add_sized(
                        ui.available_size(),
                        egui::TextEdit::singleline(&mut self.board_input)
                            .hint_text("Enter board url..."),
                    );

                    if button_res.clicked()
                        || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        board_input = Some(self.board_input.clone());
                        self.board_input.clear();
                    }
                });
            });

            // Reactors
            if let Some(new_thread) = thread_to_add {
                self.state.add_new_thread(new_thread);
            }

            if let Some(i) = index_to_delete {
                self.state.remove_thread_by_index(i);
            }

            if let Some(link) = board_input {
                self.state.set_target(link);
            }

            if let Some(i) = load_captcha_id {
                self.active_index = Some(i);
                self.state.load_captcha();
            }

            if let Some(i) = self.active_index
                && let Some(c_text) = captcha_text
            {
                self.state.bump_post(i, c_text);
                self.active_index = None;
                self.captcha_input = String::new();
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 780.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Autobump :3",
        options,
        Box::new(|_| Ok(Box::new(BumpApp::new()))),
    )?;

    Ok(())
}
