use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppData {
    pub threads: Vec<String>,
    pub proxies: Vec<String>,
}

#[derive(Deserialize)]
pub struct CaptchaIdResponse {
    pub id: String,
}
