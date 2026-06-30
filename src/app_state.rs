use crate::data_holders::AppData;
use crossbeam_channel::{Receiver, Sender, unbounded};
use reqwest::Error;
use std::sync::mpsc;

use crate::data_holders::CaptchaIdResponse;

pub struct AppState {
    pub app_data: AppData,
    // captcha txrx
    pub tx_captcha: Sender<Vec<u8>>,
    pub rx_captcha: Receiver<Vec<u8>>,
}

impl AppState {
    pub fn new() -> Self {
        let app_data = Self::load_data();
        let (tx_captcha, rx_captcha) = unbounded::<Vec<u8>>();

        println!("app_data: {}", app_data.host);

        Self {
            app_data,
            tx_captcha,
            rx_captcha,
        }
    }

    fn load_data() -> AppData {
        match std::fs::read_to_string("data.json") {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => AppData::default(),
        }
    }

    fn save_data(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.app_data) {
            std::fs::write("data.json", json).expect("Failed to write data");
        }
    }

    pub fn add_new_thread(&mut self, new_thread: String) {
        self.app_data.threads.push(new_thread);

        self.save_data();
    }

    pub fn remove_thread_by_index(&mut self, i: usize) {
        self.app_data.threads.remove(i);

        self.save_data();
    }

    pub fn load_captcha(&self) {
        let mut tx_captcha_handler = self.tx_captcha.clone();
        let host = self.app_data.host.clone();

        tokio::spawn(async move {
            let bytes = Self::perform_load_captcha(host).await;

            if bytes.is_ok() {
                tx_captcha_handler.send(bytes.unwrap().clone()).unwrap();
            }
        });
    }

    async fn perform_load_captcha(host: String) -> anyhow::Result<Vec<u8>> {
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
}
