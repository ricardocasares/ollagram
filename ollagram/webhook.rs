use crate::{
    config, ollagram,
    storage::InMemoryStorage,
    telegram::{Telegram, Update},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::LazyLock;
use vercel_runtime::{Error, Request, Response};

const TELEGRAM_SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

static STORAGE: LazyLock<InMemoryStorage> = LazyLock::new(InMemoryStorage::new);

pub async fn handler(req: Request) -> Result<Response<Value>, Error> {
    if req.method().as_str() != "POST" {
        return response(405, json!({ "ok": false }));
    }

    let cfg = config::from_env()?;

    if !has_valid_secret(&req, &cfg.webhook_secret) {
        return response(401, json!({ "ok": false }));
    }

    let telegram = Telegram::new(cfg.telegram_token.clone());
    let body = req.into_body().collect().await?.to_bytes();
    let update: Update = serde_json::from_slice(&body)?;

    ollagram::process(update, &*STORAGE, &cfg, &telegram).await?;

    response(200, json!({ "ok": true }))
}

fn has_valid_secret(req: &Request, webhook_secret: &str) -> bool {
    req.headers()
        .get(TELEGRAM_SECRET_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == webhook_secret)
        .unwrap_or(false)
}

fn response(status: u16, body: Value) -> Result<Response<Value>, Error> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(body)
        .map_err(Into::into)
}
