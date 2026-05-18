use serde::{Deserialize, Deserializer, Serialize, de};
use std::{error::Error, fmt, future::Future, num::NonZeroI64, time::Duration};

const TELEGRAM_API_URL: &str = "https://api.telegram.org";
const CHAT_ACTION_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub struct Telegram {
    token: String,
    client: reqwest::Client,
}

#[derive(Debug)]
pub enum TelegramClientError {
    Request(reqwest::Error),
    Api(TelegramError),
}

impl TelegramClientError {
    pub fn is_message_not_modified(&self) -> bool {
        match self {
            Self::Api(error) => error.is_message_not_modified(),
            Self::Request(_error) => false,
        }
    }
}

impl fmt::Display for TelegramClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => write!(formatter, "telegram request failed: {error}"),
            Self::Api(error) => write!(formatter, "telegram api failed: {error:?}"),
        }
    }
}

impl Error for TelegramClientError {}

#[derive(Debug, Serialize)]
struct GetUpdatesRequest {
    timeout: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    chat_id: ChatId,
    text: String,
    parse_mode: ParseMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_parameters: Option<ReplyParameters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<ReplyMarkup>,
}

#[derive(Debug, Serialize)]
struct SendMessageDraftRequest {
    chat_id: ChatId,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    draft_id: MessageDraftId,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<ParseMode>,
}

#[derive(Debug, Serialize)]
struct SendChatActionRequest {
    chat_id: ChatId,
    action: ChatAction,
}

#[derive(Debug, Serialize)]
struct AnswerCallbackQueryRequest {
    callback_query_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_alert: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Serialize)]
struct EditMessageReplyMarkupRequest {
    chat_id: ChatId,
    message_id: i64,
    reply_markup: InlineKeyboardMarkup,
}

#[derive(Debug, Serialize)]
enum ParseMode {
    #[serde(rename = "HTML")]
    Html,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackQueryAnswer {
    Empty,
    Notification { text: String },
    Alert { text: String },
    Url { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendMessageOptions {
    Plain,
    InlineKeyboard(InlineKeyboardMarkup),
    ReplyKeyboard(ReplyKeyboardMarkup),
    Reply(ReplyParameters),
    ReplyWithInlineKeyboard {
        reply: ReplyParameters,
        keyboard: InlineKeyboardMarkup,
    },
    ForceReply(ForceReply),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GetUpdatesOptions {
    DefaultTimeout { offset: Option<i64> },
    Timeout { offset: Option<i64>, timeout: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAction {
    Typing,
    UploadPhoto,
    RecordVideo,
    UploadVideo,
    RecordVoice,
    UploadVoice,
    UploadDocument,
    ChooseSticker,
    FindLocation,
    RecordVideoNote,
    UploadVideoNote,
}

impl GetUpdatesOptions {
    fn into_request(self) -> GetUpdatesRequest {
        match self {
            Self::DefaultTimeout { offset } => GetUpdatesRequest {
                timeout: 30,
                offset,
            },
            Self::Timeout { offset, timeout } => GetUpdatesRequest { timeout, offset },
        }
    }
}

impl SendMessageOptions {
    fn into_request_parts(self) -> (Option<ReplyParameters>, Option<ReplyMarkup>) {
        match self {
            Self::Plain => (None, None),
            Self::InlineKeyboard(keyboard) => (None, Some(ReplyMarkup::InlineKeyboard(keyboard))),
            Self::ReplyKeyboard(keyboard) => (None, Some(ReplyMarkup::ReplyKeyboard(keyboard))),
            Self::Reply(reply) => (Some(reply), None),
            Self::ReplyWithInlineKeyboard { reply, keyboard } => {
                (Some(reply), Some(ReplyMarkup::InlineKeyboard(keyboard)))
            }
            Self::ForceReply(force_reply) => (None, Some(ReplyMarkup::ForceReply(force_reply))),
        }
    }
}

impl CallbackQueryAnswer {
    fn into_request(self, callback_query_id: String) -> AnswerCallbackQueryRequest {
        match self {
            Self::Empty => AnswerCallbackQueryRequest {
                callback_query_id,
                text: None,
                show_alert: None,
                url: None,
            },
            Self::Notification { text } => AnswerCallbackQueryRequest {
                callback_query_id,
                text: Some(text),
                show_alert: None,
                url: None,
            },
            Self::Alert { text } => AnswerCallbackQueryRequest {
                callback_query_id,
                text: Some(text),
                show_alert: Some(true),
                url: None,
            },
            Self::Url { url } => AnswerCallbackQueryRequest {
                callback_query_id,
                text: None,
                show_alert: None,
                url: Some(url),
            },
        }
    }
}

impl Telegram {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
        }
    }

    pub async fn send_message(
        &self,
        chat_id: ChatId,
        text: String,
        options: SendMessageOptions,
    ) -> Result<Message, TelegramClientError> {
        let (reply_parameters, reply_markup) = options.into_request_parts();
        let request = SendMessageRequest {
            chat_id,
            text,
            parse_mode: ParseMode::Html,
            reply_parameters,
            reply_markup,
        };

        self.post("sendMessage", &request).await
    }

    pub async fn send_message_draft(
        &self,
        chat_id: ChatId,
        draft_id: MessageDraftId,
        text: Option<String>,
        message_thread_id: Option<i64>,
    ) -> Result<bool, TelegramClientError> {
        let parse_mode = text.as_ref().map(|_text| ParseMode::Html);
        let request = SendMessageDraftRequest {
            chat_id,
            message_thread_id,
            draft_id,
            text,
            parse_mode,
        };

        self.post("sendMessageDraft", &request).await
    }

    pub async fn send_chat_action(
        &self,
        chat_id: ChatId,
        action: ChatAction,
    ) -> Result<bool, TelegramClientError> {
        let request = SendChatActionRequest { chat_id, action };

        self.post("sendChatAction", &request).await
    }

    pub async fn with_chat_action<T, F>(&self, chat_id: ChatId, action: ChatAction, work: F) -> T
    where
        F: Future<Output = T>,
    {
        let telegram = self.clone();

        with_interval(
            CHAT_ACTION_INTERVAL,
            move || {
                let telegram = telegram.clone();

                async move {
                    match telegram.send_chat_action(chat_id, action).await {
                        Ok(_sent) => {}
                        Err(error) => {
                            log::warn!("chat action {action:?} failed: {error}");
                        }
                    }
                }
            },
            work,
        )
        .await
    }

    pub async fn get_updates(
        &self,
        options: GetUpdatesOptions,
    ) -> Result<Vec<Update>, TelegramClientError> {
        let request = options.into_request();

        self.post("getUpdates", &request).await
    }

    pub async fn answer_callback_query(
        &self,
        callback_query_id: String,
        answer: CallbackQueryAnswer,
    ) -> Result<bool, TelegramClientError> {
        let request = answer.into_request(callback_query_id);

        self.post("answerCallbackQuery", &request).await
    }

    pub async fn edit_message_reply_markup(
        &self,
        chat_id: ChatId,
        message_id: i64,
        reply_markup: InlineKeyboardMarkup,
    ) -> Result<Message, TelegramClientError> {
        let request = EditMessageReplyMarkupRequest {
            chat_id,
            message_id,
            reply_markup,
        };

        self.post("editMessageReplyMarkup", &request).await
    }

    async fn post<T, R>(&self, method: &str, request: &T) -> Result<R, TelegramClientError>
    where
        T: Serialize + ?Sized,
        R: for<'de> Deserialize<'de>,
    {
        let url = format!("{}/bot{}/{}", TELEGRAM_API_URL, self.token, method);

        loop {
            let response = self
                .client
                .post(&url)
                .json(request)
                .send()
                .await
                .map_err(TelegramClientError::Request)?
                .json::<TelegramResponse<R>>()
                .await
                .map_err(TelegramClientError::Request)?;

            match response {
                TelegramResponse::Ok { result } => return Ok(result),
                TelegramResponse::Err(error) => match error.retry_after() {
                    Some(delay) => {
                        log::warn!(
                            "telegram rate limited method {method}; retrying after {delay:?}"
                        );
                        tokio::time::sleep(delay).await;
                    }
                    None => return Err(TelegramClientError::Api(error)),
                },
            }
        }
    }
}

async fn with_interval<T, F, C, Fut>(interval: Duration, mut callback: C, work: F) -> T
where
    F: Future<Output = T>,
    C: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let task = tokio::spawn(async move {
        loop {
            callback().await;
            tokio::time::sleep(interval).await;
        }
    });

    let result = work.await;
    task.abort();
    result
}

#[derive(Debug, Clone)]
pub struct ErrorCode<const CODE: u16>;

impl<'de, const CODE: u16> Deserialize<'de> for ErrorCode<CODE> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = u16::deserialize(deserializer)?;

        match code == CODE {
            true => Ok(Self),
            false => Err(de::Error::custom(format!("expected error code {}", CODE))),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TelegramResponse<T> {
    Ok { result: T },

    Err(TelegramError),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TelegramError {
    RetryAfter {
        error_code: ErrorCode<429>,
        description: String,
        parameters: RetryAfterParameters,
    },

    Migrate {
        error_code: ErrorCode<400>,
        description: String,
        parameters: MigrateParameters,
    },

    Conflict {
        error_code: ErrorCode<409>,
        description: String,
    },

    Other {
        error_code: u16,
        description: String,
    },
}

impl TelegramError {
    fn is_message_not_modified(&self) -> bool {
        match self {
            Self::Other {
                error_code,
                description,
            } => *error_code == 400 && description.contains("Bad Request: message is not modified"),
            Self::RetryAfter {
                error_code: _error_code,
                description: _description,
                parameters: _parameters,
            } => false,
            Self::Migrate {
                error_code: _error_code,
                description: _description,
                parameters: _parameters,
            } => false,
            Self::Conflict {
                error_code: _error_code,
                description: _description,
            } => false,
        }
    }

    fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RetryAfter {
                error_code: _error_code,
                description: _description,
                parameters,
            } => Some(Duration::from_secs(parameters.retry_after)),

            Self::Migrate {
                error_code: _error_code,
                description: _description,
                parameters: _parameters,
            } => None,

            Self::Conflict {
                error_code: _error_code,
                description: _description,
            } => None,

            Self::Other {
                error_code: _error_code,
                description: _description,
            } => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetryAfterParameters {
    pub retry_after: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MigrateParameters {
    pub migrate_to_chat_id: i64,
}

pub type ChatId = i64;
pub type MessageDraftId = NonZeroI64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplyParameters {
    pub message_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<ChatId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_sending_without_reply: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ReplyMarkup {
    InlineKeyboard(InlineKeyboardMarkup),
    ReplyKeyboard(ReplyKeyboardMarkup),
    ForceReply(ForceReply),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InlineKeyboardButton {
    CallbackDataStyled {
        text: String,
        callback_data: String,
        style: InlineKeyboardButtonStyle,
    },
    CallbackData {
        text: String,
        callback_data: String,
    },
    Url {
        text: String,
        url: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InlineKeyboardButtonStyle {
    Danger,
    Primary,
    Success,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplyKeyboardMarkup {
    pub keyboard: Vec<Vec<KeyboardButton>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resize_keyboard: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub one_time_keyboard: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_field_placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selective: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum KeyboardButton {
    Text(String),
    Styled {
        text: String,
        style: InlineKeyboardButtonStyle,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForceReply {
    pub force_reply: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_field_placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selective: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Update {
    pub update_id: i64,

    #[serde(flatten)]
    pub tag: UpdateTag,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum UpdateTag {
    Message { message: Message },
    CallbackQuery { callback_query: CallbackQuery },
    Unsupported(serde_json::Value),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub message_id: i64,
    #[serde(rename = "from")]
    pub from: Option<TelegramUser>,
    pub chat: Chat,

    #[serde(flatten)]
    pub tag: MessageTag,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramUser {
    pub first_name: String,
    pub language_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Chat {
    pub id: ChatId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MessageTag {
    Photo {
        photo: Vec<PhotoSize>,
        caption: Option<String>,
    },
    Document {
        document: Document,
        caption: Option<String>,
    },
    Location {
        location: Location,
    },
    Text {
        text: String,
    },
    Unsupported(serde_json::Value),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PhotoSize {
    file_id: String,
    file_unique_id: String,
    width: i64,
    height: i64,
    file_size: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Document {
    file_id: String,
    file_unique_id: String,
    thumbnail: Option<PhotoSize>,
    file_name: Option<String>,
    mime_type: Option<String>,
    file_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CallbackQuery {
    pub id: String,
    #[serde(rename = "from")]
    pub from: TelegramUser,
    #[serde(default, deserialize_with = "deserialize_optional_message")]
    pub message: Option<Message>,
    pub data: Option<String>,
}

fn deserialize_optional_message<'de, D>(deserializer: D) -> Result<Option<Message>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;

    match value {
        Some(value) => Ok(serde_json::from_value(value)
            .ok()
            .and_then(supported_callback_message)),
        None => Ok(None),
    }
}

fn supported_callback_message(message: Message) -> Option<Message> {
    match &message.tag {
        MessageTag::Photo {
            photo: _photo,
            caption: _caption,
        } => Some(message),
        MessageTag::Document {
            document: _document,
            caption: _caption,
        } => Some(message),
        MessageTag::Location {
            location: _location,
        } => Some(message),
        MessageTag::Text { text: _text } => Some(message),
        MessageTag::Unsupported(_value) => None,
    }
}

//////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::{
        CHAT_ACTION_INTERVAL, CallbackQueryAnswer, ChatAction, EditMessageReplyMarkupRequest,
        ErrorCode, ForceReply, GetUpdatesOptions, InlineKeyboardButton, InlineKeyboardMarkup,
        KeyboardButton, MessageDraftId, MessageTag, ParseMode, ReplyKeyboardMarkup,
        ReplyParameters, RetryAfterParameters, SendChatActionRequest, SendMessageDraftRequest,
        SendMessageOptions, SendMessageRequest, TelegramError, TelegramResponse, Update, UpdateTag,
        with_interval,
    };
    use serde_json::json;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::time;

    #[test]
    fn deserializes_successful_updates_response() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 1,
                    "message": {
                        "message_id": 2,
                        "from": {
                            "id": 100,
                            "is_bot": false,
                            "first_name": "User",
                            "language_code": "en"
                        },
                        "chat": {
                            "id": 42
                        },
                        "text": "hello"
                    }
                }
            ]
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result } => {
                assert_eq!(result.len(), 1);
                assert_eq!(result[0].update_id, 1);

                match &result[0].tag {
                    UpdateTag::Message { message } => match &message.tag {
                        MessageTag::Photo {
                            photo: _,
                            caption: _,
                        } => panic!("expected text message"),
                        MessageTag::Document {
                            document: _,
                            caption: _,
                        } => panic!("expected text message"),
                        MessageTag::Location { location: _ } => panic!("expected text message"),
                        MessageTag::Text { text } => {
                            assert_eq!(message.chat.id, 42);
                            match &message.from {
                                Some(user) => {
                                    assert_eq!(user.first_name, "User");
                                    assert_eq!(user.language_code.as_deref(), Some("en"));
                                }
                                None => panic!("expected message user"),
                            }
                            assert_eq!(text, "hello");
                        }
                        MessageTag::Unsupported(_value) => panic!("expected text message"),
                    },
                    UpdateTag::CallbackQuery { callback_query: _ } => {
                        panic!("expected message update")
                    }
                    UpdateTag::Unsupported(_value) => panic!("expected message update"),
                }
            }
            TelegramResponse::Err(_) => panic!("expected successful response"),
        }
    }

    #[test]
    fn deserializes_callback_query_with_accessible_message() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 1,
                    "callback_query": {
                        "id": "callback-1",
                        "from": {
                            "id": 100,
                            "is_bot": false,
                            "first_name": "User"
                        },
                        "message": {
                            "message_id": 2,
                            "chat": {
                                "id": 42
                            },
                            "text": "choose"
                        },
                        "chat_instance": "chat-instance",
                        "data": "Summarize this"
                    }
                }
            ]
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result } => match &result[0].tag {
                UpdateTag::Message { message: _message } => panic!("expected callback query"),
                UpdateTag::Unsupported(_value) => panic!("expected callback query"),
                UpdateTag::CallbackQuery { callback_query } => {
                    assert_eq!(callback_query.id, "callback-1");
                    assert_eq!(callback_query.from.first_name, "User");
                    assert_eq!(callback_query.from.language_code, None);
                    assert_eq!(callback_query.data, Some(String::from("Summarize this")));
                    match &callback_query.message {
                        Some(message) => {
                            assert_eq!(message.chat.id, 42);
                            assert_eq!(message.message_id, 2);
                        }
                        None => panic!("expected callback message"),
                    }
                }
            },
            TelegramResponse::Err(_error) => panic!("expected successful response"),
        }
    }

    #[test]
    fn deserializes_callback_query_with_inaccessible_message_as_none() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 1,
                    "callback_query": {
                        "id": "callback-1",
                        "from": {
                            "id": 100,
                            "is_bot": false,
                            "first_name": "User"
                        },
                        "message": {
                            "chat": {
                                "id": 42
                            },
                            "message_id": 2,
                            "date": 0
                        },
                        "chat_instance": "chat-instance",
                        "data": "Summarize this"
                    }
                }
            ]
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result } => match &result[0].tag {
                UpdateTag::Message { message: _message } => panic!("expected callback query"),
                UpdateTag::Unsupported(_value) => panic!("expected callback query"),
                UpdateTag::CallbackQuery { callback_query } => {
                    assert_eq!(callback_query.data, Some(String::from("Summarize this")));
                    assert!(callback_query.message.is_none());
                }
            },
            TelegramResponse::Err(_error) => panic!("expected successful response"),
        }
    }

    #[test]
    fn deserializes_unsupported_message_update() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 1,
                    "message": {
                        "message_id": 2,
                        "chat": {
                            "id": 42
                        },
                        "sticker": {
                            "file_id": "sticker-file",
                            "file_unique_id": "sticker-unique",
                            "type": "regular",
                            "width": 512,
                            "height": 512,
                            "is_animated": false,
                            "is_video": false
                        }
                    }
                }
            ]
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result } => match &result[0].tag {
                UpdateTag::Message { message } => match &message.tag {
                    MessageTag::Photo {
                        photo: _photo,
                        caption: _caption,
                    } => panic!("expected unsupported message"),
                    MessageTag::Document {
                        document: _document,
                        caption: _caption,
                    } => panic!("expected unsupported message"),
                    MessageTag::Location {
                        location: _location,
                    } => panic!("expected unsupported message"),
                    MessageTag::Text { text: _text } => panic!("expected unsupported message"),
                    MessageTag::Unsupported(value) => {
                        assert_eq!(message.chat.id, 42);
                        assert!(value.get("sticker").is_some());
                    }
                },
                UpdateTag::CallbackQuery {
                    callback_query: _callback_query,
                } => panic!("expected message update"),
                UpdateTag::Unsupported(_value) => panic!("expected message update"),
            },
            TelegramResponse::Err(_error) => panic!("expected successful response"),
        }
    }

    #[test]
    fn deserializes_unsupported_update() {
        let json = r#"{
            "ok": true,
            "result": [
                {
                    "update_id": 1,
                    "edited_message": {
                        "message_id": 2,
                        "chat": {
                            "id": 42
                        },
                        "text": "edited"
                    }
                }
            ]
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result } => match &result[0].tag {
                UpdateTag::Message { message: _message } => panic!("expected unsupported update"),
                UpdateTag::CallbackQuery {
                    callback_query: _callback_query,
                } => panic!("expected unsupported update"),
                UpdateTag::Unsupported(value) => {
                    assert!(value.get("edited_message").is_some());
                }
            },
            TelegramResponse::Err(_error) => panic!("expected successful response"),
        }
    }

    #[test]
    fn deserializes_retry_after_error_response() {
        let json = r#"{
            "ok": false,
            "error_code": 429,
            "description": "Too Many Requests: retry after 3",
            "parameters": {
                "retry_after": 3
            }
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result: _ } => panic!("expected error response"),
            TelegramResponse::Err(error) => match error {
                TelegramError::RetryAfter {
                    error_code: _,
                    description,
                    parameters,
                } => {
                    assert_eq!(description, "Too Many Requests: retry after 3");
                    assert_eq!(parameters.retry_after, 3);
                }
                TelegramError::Migrate {
                    error_code: _,
                    description: _,
                    parameters: _,
                } => panic!("expected retry after error"),
                TelegramError::Conflict {
                    error_code: _,
                    description: _,
                } => panic!("expected retry after error"),
                TelegramError::Other {
                    error_code: _,
                    description: _,
                } => panic!("expected retry after error"),
            },
        }
    }

    #[test]
    fn deserializes_migrate_error_response() {
        let json = r#"{
            "ok": false,
            "error_code": 400,
            "description": "Bad Request: group chat was migrated to a supergroup chat",
            "parameters": {
                "migrate_to_chat_id": -123456789
            }
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result: _ } => panic!("expected error response"),
            TelegramResponse::Err(error) => match error {
                TelegramError::RetryAfter {
                    error_code: _,
                    description: _,
                    parameters: _,
                } => panic!("expected migrate error"),
                TelegramError::Migrate {
                    error_code: _,
                    description,
                    parameters,
                } => {
                    assert_eq!(
                        description,
                        "Bad Request: group chat was migrated to a supergroup chat"
                    );
                    assert_eq!(parameters.migrate_to_chat_id, -123456789);
                }
                TelegramError::Conflict {
                    error_code: _,
                    description: _,
                } => panic!("expected migrate error"),
                TelegramError::Other {
                    error_code: _,
                    description: _,
                } => panic!("expected migrate error"),
            },
        }
    }

    #[test]
    fn deserializes_webhook_conflict_error_response() {
        let json = r#"{
            "ok": false,
            "error_code": 409,
            "description": "Conflict: can't use getUpdates method while webhook is active; use deleteWebhook to delete the webhook first"
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result: _ } => panic!("expected error response"),
            TelegramResponse::Err(error) => assert_conflict(error),
        }
    }

    #[test]
    fn deserializes_long_poll_conflict_error_response() {
        let json = r#"{
            "ok": false,
            "error_code": 409,
            "description": "Conflict: terminated by other getUpdates request; make sure that only one bot instance is running"
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result: _ } => panic!("expected error response"),
            TelegramResponse::Err(error) => assert_conflict(error),
        }
    }

    #[test]
    fn deserializes_other_error_response() {
        let json = r#"{
            "ok": false,
            "error_code": 401,
            "description": "Unauthorized"
        }"#;

        let response: TelegramResponse<Vec<Update>> = serde_json::from_str(json).unwrap();

        match response {
            TelegramResponse::Ok { result: _ } => panic!("expected error response"),
            TelegramResponse::Err(error) => match error {
                TelegramError::RetryAfter {
                    error_code: _,
                    description: _,
                    parameters: _,
                } => panic!("expected other error"),
                TelegramError::Migrate {
                    error_code: _,
                    description: _,
                    parameters: _,
                } => panic!("expected other error"),
                TelegramError::Conflict {
                    error_code: _,
                    description: _,
                } => panic!("expected other error"),
                TelegramError::Other {
                    error_code,
                    description,
                } => {
                    assert_eq!(error_code, 401);
                    assert_eq!(description, "Unauthorized");
                }
            },
        }
    }

    #[test]
    fn rejects_marker_mismatches() {
        assert!(serde_json::from_str::<ErrorCode<429>>("400").is_err());
    }

    #[test]
    fn extracts_retry_after_delay() {
        let error = TelegramError::RetryAfter {
            error_code: ErrorCode,
            description: String::from("Too Many Requests: retry after 3"),
            parameters: RetryAfterParameters { retry_after: 3 },
        };

        assert_eq!(error.retry_after(), Some(Duration::from_secs(3)));
    }

    #[test]
    fn serializes_long_poll_get_updates() {
        let request = GetUpdatesOptions::DefaultTimeout { offset: Some(3) }.into_request();

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "timeout": 30,
                "offset": 3
            })
        );
    }

    #[test]
    fn serializes_immediate_get_updates() {
        let request = GetUpdatesOptions::Timeout {
            offset: None,
            timeout: 0,
        }
        .into_request();

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "timeout": 0
            })
        );
    }

    #[test]
    fn serializes_send_message_without_reply_markup() {
        let request = SendMessageRequest {
            chat_id: 42,
            text: String::from("hello"),
            parse_mode: ParseMode::Html,
            reply_parameters: None,
            reply_markup: None,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "text": "hello",
                "parse_mode": "HTML"
            })
        );
    }

    #[test]
    fn serializes_send_message_with_inline_keyboard() {
        let request = SendMessageRequest {
            chat_id: 42,
            text: String::from("choose"),
            parse_mode: ParseMode::Html,
            reply_parameters: None,
            reply_markup: SendMessageOptions::InlineKeyboard(InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![InlineKeyboardButton::CallbackData {
                        text: String::from("Use this"),
                        callback_data: String::from("use:this"),
                    }],
                    vec![InlineKeyboardButton::CallbackDataStyled {
                        text: String::from("Delete"),
                        callback_data: String::from("delete:this"),
                        style: super::InlineKeyboardButtonStyle::Danger,
                    }],
                    vec![InlineKeyboardButton::Url {
                        text: String::from("Docs"),
                        url: String::from("https://core.telegram.org/bots/api"),
                    }],
                ],
            })
            .into_request_parts()
            .1,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "text": "choose",
                "parse_mode": "HTML",
                "reply_markup": {
                    "inline_keyboard": [
                        [
                            {
                                "text": "Use this",
                                "callback_data": "use:this"
                            }
                        ],
                        [
                            {
                                "text": "Delete",
                                "callback_data": "delete:this",
                                "style": "danger"
                            }
                        ],
                        [
                            {
                                "text": "Docs",
                                "url": "https://core.telegram.org/bots/api"
                            }
                        ]
                    ]
                }
            })
        );
    }

    #[test]
    fn serializes_edit_message_reply_markup() {
        let request = EditMessageReplyMarkupRequest {
            chat_id: 42,
            message_id: 7,
            reply_markup: InlineKeyboardMarkup {
                inline_keyboard: vec![vec![InlineKeyboardButton::CallbackData {
                    text: String::from("Summarize"),
                    callback_data: String::from("Summarize this"),
                }]],
            },
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "message_id": 7,
                "reply_markup": {
                    "inline_keyboard": [
                        [
                            {
                                "text": "Summarize",
                                "callback_data": "Summarize this"
                            }
                        ]
                    ]
                }
            })
        );
    }

    #[test]
    fn serializes_send_message_with_reply_parameters() {
        let (reply_parameters, reply_markup) = SendMessageOptions::Reply(ReplyParameters {
            message_id: 7,
            chat_id: None,
            allow_sending_without_reply: Some(true),
        })
        .into_request_parts();

        let request = SendMessageRequest {
            chat_id: 42,
            text: String::from("reply"),
            parse_mode: ParseMode::Html,
            reply_parameters,
            reply_markup,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "text": "reply",
                "parse_mode": "HTML",
                "reply_parameters": {
                    "message_id": 7,
                    "allow_sending_without_reply": true
                }
            })
        );
    }

    #[test]
    fn serializes_send_message_with_reply_keyboard() {
        let (reply_parameters, reply_markup) =
            SendMessageOptions::ReplyKeyboard(ReplyKeyboardMarkup {
                keyboard: vec![vec![
                    KeyboardButton::Styled {
                        text: String::from("Yes"),
                        style: super::InlineKeyboardButtonStyle::Primary,
                    },
                    KeyboardButton::Text(String::from("No")),
                ]],
                resize_keyboard: Some(true),
                one_time_keyboard: Some(true),
                input_field_placeholder: Some(String::from("Choose")),
                selective: None,
            })
            .into_request_parts();

        let request = SendMessageRequest {
            chat_id: 42,
            text: String::from("choose"),
            parse_mode: ParseMode::Html,
            reply_parameters,
            reply_markup,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "text": "choose",
                "parse_mode": "HTML",
                "reply_markup": {
                    "keyboard": [
                        [
                            {
                                "text": "Yes",
                                "style": "primary"
                            },
                            "No"
                        ]
                    ],
                    "resize_keyboard": true,
                    "one_time_keyboard": true,
                    "input_field_placeholder": "Choose"
                }
            })
        );
    }

    #[test]
    fn serializes_send_message_with_force_reply() {
        let (reply_parameters, reply_markup) = SendMessageOptions::ForceReply(ForceReply {
            force_reply: true,
            input_field_placeholder: Some(String::from("Type here")),
            selective: None,
        })
        .into_request_parts();

        let request = SendMessageRequest {
            chat_id: 42,
            text: String::from("question"),
            parse_mode: ParseMode::Html,
            reply_parameters,
            reply_markup,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "text": "question",
                "parse_mode": "HTML",
                "reply_markup": {
                    "force_reply": true,
                    "input_field_placeholder": "Type here"
                }
            })
        );
    }

    #[test]
    fn serializes_send_message_draft_with_text() {
        let request = SendMessageDraftRequest {
            chat_id: 42,
            message_thread_id: Some(7),
            draft_id: MessageDraftId::new(1).unwrap(),
            text: Some(String::from("partial")),
            parse_mode: Some(ParseMode::Html),
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "message_thread_id": 7,
                "draft_id": 1,
                "text": "partial",
                "parse_mode": "HTML"
            })
        );
    }

    #[test]
    fn serializes_send_message_draft_placeholder() {
        let request = SendMessageDraftRequest {
            chat_id: 42,
            message_thread_id: None,
            draft_id: MessageDraftId::new(1).unwrap(),
            text: Some(String::new()),
            parse_mode: Some(ParseMode::Html),
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "draft_id": 1,
                "text": "",
                "parse_mode": "HTML"
            })
        );
    }

    #[test]
    fn serializes_send_chat_action() {
        let request = SendChatActionRequest {
            chat_id: 42,
            action: ChatAction::Typing,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "action": "typing"
            })
        );
    }

    #[test]
    fn serializes_send_chat_action_upload_document() {
        let request = SendChatActionRequest {
            chat_id: 42,
            action: ChatAction::UploadDocument,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "action": "upload_document"
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn interval_runs_immediately_and_repeats_until_work_finishes() {
        let tick_count = Arc::new(AtomicUsize::new(0));
        let callback_tick_count = tick_count.clone();

        let result = with_interval(
            CHAT_ACTION_INTERVAL,
            move || {
                let callback_tick_count = callback_tick_count.clone();

                async move {
                    callback_tick_count.fetch_add(1, Ordering::SeqCst);
                }
            },
            async {
                tokio::task::yield_now().await;
                assert_eq!(tick_count.load(Ordering::SeqCst), 1);

                time::advance(CHAT_ACTION_INTERVAL).await;
                tokio::task::yield_now().await;
                assert_eq!(tick_count.load(Ordering::SeqCst), 2);

                "done"
            },
        )
        .await;

        assert_eq!(result, "done");
    }

    #[tokio::test(start_paused = true)]
    async fn interval_stops_after_work_finishes() {
        let tick_count = Arc::new(AtomicUsize::new(0));
        let callback_tick_count = tick_count.clone();

        with_interval(
            CHAT_ACTION_INTERVAL,
            move || {
                let callback_tick_count = callback_tick_count.clone();

                async move {
                    callback_tick_count.fetch_add(1, Ordering::SeqCst);
                }
            },
            async {
                tokio::task::yield_now().await;
            },
        )
        .await;

        assert_eq!(tick_count.load(Ordering::SeqCst), 1);

        time::advance(CHAT_ACTION_INTERVAL + CHAT_ACTION_INTERVAL).await;
        tokio::task::yield_now().await;

        assert_eq!(tick_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn omits_absent_message_draft_text() {
        let request = SendMessageDraftRequest {
            chat_id: 42,
            message_thread_id: None,
            draft_id: MessageDraftId::new(1).unwrap(),
            text: None,
            parse_mode: None,
        };

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "chat_id": 42,
                "draft_id": 1
            })
        );
    }

    #[test]
    fn serializes_empty_answer_callback_query() {
        let request = CallbackQueryAnswer::Empty.into_request(String::from("callback-id"));

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "callback_query_id": "callback-id"
            })
        );
    }

    #[test]
    fn serializes_notification_answer_callback_query() {
        let request = CallbackQueryAnswer::Notification {
            text: String::from("Saved"),
        }
        .into_request(String::from("callback-id"));

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "callback_query_id": "callback-id",
                "text": "Saved"
            })
        );
    }

    #[test]
    fn serializes_alert_answer_callback_query() {
        let request = CallbackQueryAnswer::Alert {
            text: String::from("Needs attention"),
        }
        .into_request(String::from("callback-id"));

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "callback_query_id": "callback-id",
                "text": "Needs attention",
                "show_alert": true
            })
        );
    }

    #[test]
    fn serializes_url_answer_callback_query() {
        let request = CallbackQueryAnswer::Url {
            url: String::from("https://example.com"),
        }
        .into_request(String::from("callback-id"));

        let json = serde_json::to_value(request).unwrap();

        assert_eq!(
            json,
            json!({
                "callback_query_id": "callback-id",
                "url": "https://example.com"
            })
        );
    }

    fn assert_conflict(error: TelegramError) {
        match error {
            TelegramError::RetryAfter {
                error_code: _,
                description: _,
                parameters: _,
            } => panic!("expected conflict error"),
            TelegramError::Migrate {
                error_code: _,
                description: _,
                parameters: _,
            } => panic!("expected conflict error"),
            TelegramError::Conflict {
                error_code: _,
                description,
            } => assert!(description.starts_with("Conflict:")),
            TelegramError::Other {
                error_code: _,
                description: _,
            } => panic!("expected conflict error"),
        }
    }
}
