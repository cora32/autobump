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
    texture: Option<egui::TextureHandle>,
}

impl BumpApp {
    fn new() -> Self {
        let state = AppState::new();

        Self {
            state,
            new_thread_input: String::new(),
            texture: None,
        }
    }

    fn link(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        thread: &str,
        rx_captcha: Receiver<Vec<u8>>,
        texture: &mut Option<egui::TextureHandle>,
        on_click: impl FnOnce(),
    ) {
        ui.horizontal(|ui| {
            ui.label(thread);

            //Captcha and button
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Captcha image
                if let Ok(bytes) = rx_captcha.try_recv() {
                    // Load texture from bytes
                    load_texture_from_bytes(ctx, bytes, texture);
                }
                if let Some(texture) = &texture {
                    ui.image(texture);
                }

                //Btn
                if ui.button("❌").clicked() {
                    on_click();
                }
            });
        });
    }
}

impl eframe::App for BumpApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut thread_to_add = None;
        let mut index_to_delete = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);

            ui.vertical_centered(|ui| ui.label(egui::RichText::new("Autobump :3").size(16.0)));

            ui.add_space(10.0);

            // Drawing links
            for (i, thread) in self.state.app_data.threads.iter().enumerate() {
                let bg_color = if i % 2 == 0 {
                    egui::Color32::from_rgb(15, 15, 15) // Darker
                } else {
                    egui::Color32::from_rgb(25, 52, 25) // Slightly lighter
                };
                egui::Frame::NONE
                    .fill(bg_color) // Dark gray background
                    .inner_margin(8.0) // Padding inside the frame
                    .corner_radius(4.0) // Rounded corners
                    .show(ui, |ui| {
                        Self::link(
                            ui,
                            ctx,
                            thread,
                            self.state.rx_captcha.clone(),
                            &mut self.texture,
                            || {
                                index_to_delete = Some(i);
                            },
                        );
                        ui.separator();
                    });
            }

            // Add btn
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_thread_input);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Add").clicked() {
                        thread_to_add = Some(self.new_thread_input.clone());
                        self.new_thread_input.clear();
                    };
                });
            });

            // Savers
            if let Some(new_thread) = thread_to_add {
                self.state.add_new_thread(new_thread);
            }

            if let Some(i) = index_to_delete {
                self.state.remove_thread_by_index(i);
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
        Box::new(|cc| Ok(Box::new(BumpApp::new()))),
    )?;

    Ok(())
}
