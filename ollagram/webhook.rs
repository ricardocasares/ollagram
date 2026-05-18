use crate::{
    config, ollagram,
    storage::InMemoryStorage,
    telegram::{CallbackQuery, Telegram, Update, UpdateTag},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{fmt, sync::LazyLock};
use vercel_runtime::{AppState, Error, Request, Response};

const TELEGRAM_SECRET_HEADER: &str = "x-telegram-bot-api-secret-token";

static STORAGE: LazyLock<InMemoryStorage> = LazyLock::new(InMemoryStorage::new);

pub async fn handler(req: Request, state: AppState) -> Result<Response<Value>, Error> {
    if req.method().as_str() != "POST" {
        return response(405, json!({ "ok": false }));
    }

    let cfg = match config::from_env() {
        Ok(cfg) => cfg,
        Err(error) => {
            state
                .log_context
                .error(&format!("webhook config load failed: {error}"));
            return Err(error.into());
        }
    };

    if !has_valid_secret(&req, &cfg.webhook_secret) {
        return response(401, json!({ "ok": false }));
    }

    let telegram = Telegram::new(cfg.telegram_token.clone());
    let body = match req.into_body().collect().await {
        Ok(body) => body.to_bytes(),
        Err(error) => {
            state
                .log_context
                .error(&format!("webhook body read failed: {error}"));
            return Err(error.into());
        }
    };
    let update: Update = match serde_json::from_slice(&body) {
        Ok(update) => update,
        Err(error) => {
            state
                .log_context
                .error(&format!("webhook update parse failed: {error}"));
            return Err(error.into());
        }
    };
    let summary = update_log_summary(&update);

    match ollagram::process(update, &*STORAGE, &cfg, &telegram).await {
        Ok(()) => {}
        Err(error) => {
            state.log_context.error(&format!(
                "webhook update processing failed: {summary}: {error}"
            ));
            return Err(error.into());
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateLogSummary {
    update_id: i64,
    tag: UpdateLogTag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateLogTag {
    Message {
        chat_id: i64,
        message_id: i64,
    },
    CallbackQuery {
        callback_query_id: String,
        chat_id: Option<i64>,
        message_id: Option<i64>,
    },
    Unsupported,
}

fn update_log_summary(update: &Update) -> UpdateLogSummary {
    UpdateLogSummary {
        update_id: update.update_id,
        tag: match &update.tag {
            UpdateTag::Message { message } => UpdateLogTag::Message {
                chat_id: message.chat.id,
                message_id: message.message_id,
            },
            UpdateTag::CallbackQuery { callback_query } => callback_query_log_tag(callback_query),
            UpdateTag::Unsupported(_value) => UpdateLogTag::Unsupported,
        },
    }
}

fn callback_query_log_tag(callback_query: &CallbackQuery) -> UpdateLogTag {
    let message = callback_query.message.as_ref();

    UpdateLogTag::CallbackQuery {
        callback_query_id: callback_query.id.clone(),
        chat_id: message.map(|message| message.chat.id),
        message_id: message.map(|message| message.message_id),
    }
}

impl fmt::Display for UpdateLogSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "update_id={} {}", self.update_id, self.tag)
    }
}

impl fmt::Display for UpdateLogTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message {
                chat_id,
                message_id,
            } => write!(
                formatter,
                "type=message chat_id={chat_id} message_id={message_id}"
            ),
            Self::CallbackQuery {
                callback_query_id,
                chat_id,
                message_id,
            } => match (chat_id, message_id) {
                (Some(chat_id), Some(message_id)) => write!(
                    formatter,
                    "type=callback_query callback_query_id={callback_query_id} chat_id={chat_id} message_id={message_id}"
                ),
                (Some(chat_id), None) => write!(
                    formatter,
                    "type=callback_query callback_query_id={callback_query_id} chat_id={chat_id}"
                ),
                (None, Some(message_id)) => write!(
                    formatter,
                    "type=callback_query callback_query_id={callback_query_id} message_id={message_id}"
                ),
                (None, None) => write!(
                    formatter,
                    "type=callback_query callback_query_id={callback_query_id}"
                ),
            },
            Self::Unsupported => write!(formatter, "type=unsupported"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UpdateLogTag, update_log_summary};
    use crate::telegram::Update;

    #[test]
    fn summarizes_message_update() {
        let update: Update = serde_json::from_value(serde_json::json!({
            "update_id": 1,
            "message": {
                "message_id": 2,
                "chat": { "id": 42 },
                "text": "hello"
            }
        }))
        .expect("message update should deserialize");

        let summary = update_log_summary(&update);

        assert_eq!(summary.update_id, 1);
        assert_eq!(
            summary.tag,
            UpdateLogTag::Message {
                chat_id: 42,
                message_id: 2
            }
        );
        assert_eq!(
            summary.to_string(),
            "update_id=1 type=message chat_id=42 message_id=2"
        );
    }

    #[test]
    fn summarizes_callback_query_with_accessible_message() {
        let update: Update = serde_json::from_value(serde_json::json!({
            "update_id": 3,
            "callback_query": {
                "id": "callback-1",
                "from": {
                    "id": 100,
                    "is_bot": false,
                    "first_name": "User"
                },
                "message": {
                    "message_id": 4,
                    "chat": { "id": 84 },
                    "text": "choose"
                },
                "data": "next"
            }
        }))
        .expect("callback query update should deserialize");

        let summary = update_log_summary(&update);

        assert_eq!(summary.update_id, 3);
        assert_eq!(
            summary.tag,
            UpdateLogTag::CallbackQuery {
                callback_query_id: String::from("callback-1"),
                chat_id: Some(84),
                message_id: Some(4)
            }
        );
        assert_eq!(
            summary.to_string(),
            "update_id=3 type=callback_query callback_query_id=callback-1 chat_id=84 message_id=4"
        );
    }

    #[test]
    fn summarizes_callback_query_without_accessible_message() {
        let update: Update = serde_json::from_value(serde_json::json!({
            "update_id": 5,
            "callback_query": {
                "id": "callback-2",
                "from": {
                    "id": 100,
                    "is_bot": false,
                    "first_name": "User"
                },
                "message": {
                    "message_id": 6,
                    "chat": { "id": 126 },
                    "date": 0
                },
                "data": "next"
            }
        }))
        .expect("callback query update should deserialize");

        let summary = update_log_summary(&update);

        assert_eq!(summary.update_id, 5);
        assert_eq!(
            summary.tag,
            UpdateLogTag::CallbackQuery {
                callback_query_id: String::from("callback-2"),
                chat_id: None,
                message_id: None
            }
        );
        assert_eq!(
            summary.to_string(),
            "update_id=5 type=callback_query callback_query_id=callback-2"
        );
    }

    #[test]
    fn summarizes_unsupported_update() {
        let update: Update = serde_json::from_value(serde_json::json!({
            "update_id": 7,
            "edited_message": {
                "message_id": 8,
                "chat": { "id": 168 },
                "text": "edited"
            }
        }))
        .expect("unsupported update should deserialize");

        let summary = update_log_summary(&update);

        assert_eq!(summary.update_id, 7);
        assert_eq!(summary.tag, UpdateLogTag::Unsupported);
        assert_eq!(summary.to_string(), "update_id=7 type=unsupported");
    }
}
