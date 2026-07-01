use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppData {
    pub host: String,
    pub threads: Vec<String>,
    pub proxies: Vec<String>,
}

#[derive(Deserialize)]
pub struct CaptchaIdResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct PostIdResponse {
    pub board: String,
    pub num: u32,
}
