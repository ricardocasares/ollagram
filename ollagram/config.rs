use std::env::{self, VarError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub telegram_token: String,
    pub openai_url: String,
    pub openai_key: String,
    pub webhook_secret: String,
    pub openai_model: String,
    pub system_prompt: Option<String>,
    pub system_prompt_append: Option<String>,
}

pub fn from_env() -> Result<Config, VarError> {
    let openai_url = env::var("OPENAI_URL")?;
    let openai_key = env::var("OPENAI_API_KEY")?;
    let openai_model = env::var("OPENAI_MODEL")?;
    let webhook_secret = env::var("WEBHOOK_SECRET")?;
    let telegram_token = env::var("TELEGRAM_BOT_TOKEN")?;
    let system_prompt = env::var("SYSTEM_PROMPT")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let system_prompt_append = env::var("SYSTEM_PROMPT_APPEND")
        .ok()
        .filter(|value| !value.trim().is_empty());

    Ok(Config {
        openai_url,
        openai_key,
        openai_model,
        webhook_secret,
        telegram_token,
        system_prompt,
        system_prompt_append,
    })
}
