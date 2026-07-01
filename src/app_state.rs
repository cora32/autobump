use crate::data_holders::AppData;
use crate::utils::rand_str;
use crossbeam_channel::{Receiver, Sender, unbounded};
use rand::Rng;
use regex::Regex;
use reqwest::Error;
use reqwest::StatusCode;
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, ORIGIN, REFERER};
use reqwest::multipart;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::data_holders::CaptchaIdResponse;

pub struct AppState {
    pub app_data: AppData,
    // captcha txrx
    pub tx_captcha: Sender<Vec<u8>>,
    pub rx_captcha: Receiver<Vec<u8>>,
    pub captcha_id: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn new() -> Self {
        let app_data = Self::load_data();
        let (tx_captcha, rx_captcha) = unbounded::<Vec<u8>>();
        let captcha_id = Arc::new(Mutex::new(Some(String::new())));

        println!("app_data: {}", app_data.host);

        Self {
            app_data,
            tx_captcha,
            rx_captcha,
            captcha_id,
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

    pub fn set_target(&mut self, link: String) {
        println!("Setting target: {}", link);
        self.app_data.host = link;

        self.save_data();
    }

    pub fn remove_thread_by_index(&mut self, i: usize) {
        self.app_data.threads.remove(i);

        self.save_data();
    }

    pub fn bump_post(&self, i: usize, captcha_text: String) {
        let thread_link = self.app_data.threads[i].clone();
        let host = self.app_data.host.clone();
        let captcha_id_handler = self.captcha_id.clone();

        println!("Bumping: {}", thread_link);

        tokio::spawn(async move {
            let mut captcha_id = String::new();

            if let Ok(lock) = captcha_id_handler.lock() {
                if let Some(id) = &*lock {
                    captcha_id = id.clone();
                }
            }

            let result = Self::send_post(host, thread_link, captcha_text, captcha_id).await;

            if result.is_ok() {}
        });
    }

    async fn send_post(
        host: String,
        link: String,
        captcha_text: String,
        captcha_id: String,
    ) -> anyhow::Result<bool> {
        let re = Regex::new(r"/([^/]+)/res/(\d+)\.html").unwrap();

        if let Some(caps) = re.captures(&link) {
            let board = caps[1].to_string();
            let parent = caps[2].to_string();

            println!(
                "Board: {}; parent: {}; c_id: {}, c_text: {}",
                board, parent, captcha_id, captcha_text
            );

            let client = reqwest::Client::new();

            let rnd = rand_str(10);
            let message = format!("Autobumpeeque! :3 {}", rnd);
            let form = multipart::Form::new()
                .text("task", "post")
                .text("board", board)
                .text("parent", parent)
                .text("email", "")
                .text("subject", "")
                .text("comment", message)
                .text("image", "")
                .text("captcha_id", captcha_id)
                .text("captcha_value", captcha_text)
                .text("password", "test")
                .text("json", "1");

            let mut headers = HeaderMap::new();
            headers.insert(
                "X-Requested-With",
                HeaderValue::from_static("XMLHttpRequest"),
            );

            if let Ok(referer_value) = HeaderValue::try_from(host.clone()) {
                headers.insert(ORIGIN, referer_value);
            };
            if let Ok(referer_value) = HeaderValue::try_from(link.clone()) {
                headers.insert(REFERER, referer_value);
            };
            headers.insert(COOKIE, HeaderValue::from_static("wakabastyle=Photon"));
            headers.insert(
            "User-Agent",
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:152.0) Gecko/20100101 Firefox/152.0",
            ),
        );

            let url = format!("{}cgi/posting", host);
            let response = client
                .post(url)
                .headers(headers)
                .multipart(form)
                .send()
                .await?;

            let status = response.status();
            println!("Status: {}", status.clone());
            println!("Response Body: {}", response.text().await?);

            let is_ok = status.clone() == StatusCode::OK;
            if is_ok {
                Ok(true)
            } else {
                Err(anyhow::anyhow!("Failed to post: {}", status.clone()))
            }
        } else {
            return Err(anyhow::anyhow!("Failed to parse link: {}", link));
        }
    }

    pub fn load_captcha(&self) {
        println!("Loading captcha...",);
        let tx_captcha_handler = self.tx_captcha.clone();
        let host = self.app_data.host.clone();
        let captcha_id_handler = self.captcha_id.clone();

        tokio::spawn(async move {
            let result = Self::perform_load_captcha(host).await;

            if result.is_ok() {
                println!("Captcha loaded!");
                let (bytes, captcha_id) = result.unwrap();

                if let Ok(mut lock) = captcha_id_handler.lock() {
                    *lock = Some(captcha_id);
                }

                tx_captcha_handler.send(bytes.clone()).unwrap();
            } else {
                println!("Failed to load captcha");
            }
        });
    }

    async fn perform_load_captcha(host: String) -> anyhow::Result<(Vec<u8>, String)> {
        let client = reqwest::Client::new();

        let response: CaptchaIdResponse = client
            .get(&format!("{}/cgi/captcha?task=get_id&json=1", host))
            .send()
            .await?
            .json()
            .await?;

        let image_url = format!(
            "{}/cgi/captcha?task=get_image&id={}",
            host,
            response.id.clone()
        );

        let image_bytes = client.get(image_url).send().await?.bytes().await?;

        Ok((image_bytes.to_vec(), response.id.clone()))
    }
}
