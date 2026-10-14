use std::path::PathBuf;

use crate::client::auth::OpenAiAuth;

pub struct WebSearchConfig {
    pub endpoint: String,

    pub auth: OpenAiAuth,
}

pub struct ImageGenerationConfig {
    pub api_base_url: String,

    pub auth: OpenAiAuth,

    pub save_root: PathBuf,
}
