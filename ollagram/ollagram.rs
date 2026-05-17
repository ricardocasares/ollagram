use crate::{
    config::Config,
    formatting::markdown_to_telegram_html_chunks,
    llm,
    storage::{MessageStorage, StorageError},
    telegram::{
        CallbackQuery, CallbackQueryAnswer, ChatAction, ChatId, InlineKeyboardButton,
        InlineKeyboardMarkup, Message as TelegramMessage, MessageDraftId, MessageTag,
        SendMessageOptions, Telegram, TelegramClientError, Update, UpdateTag,
    },
};
use aisdk::core::{
    LanguageModelStreamChunkType, Message as AiMessage, Messages, StreamTextResponse, UserMessage,
};
use futures::StreamExt;
use serde::Deserialize;
use std::{error::Error, fmt, time::Duration};
use tokio::time::Instant;

const DRAFT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum ProcessError {
    Storage(StorageError),
    Llm(aisdk::Error),
    StreamFailed(String),
    Telegram(TelegramClientError),
    InvalidDraftId(i64),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "storage failed: {error}"),
            Self::Llm(error) => write!(formatter, "llm failed: {error}"),
            Self::StreamFailed(error) => write!(formatter, "llm stream failed: {error}"),
            Self::Telegram(error) => write!(formatter, "telegram failed: {error}"),
            Self::InvalidDraftId(message_id) => {
                write!(
                    formatter,
                    "invalid message draft id from message {message_id}"
                )
            }
        }
    }
}

impl Error for ProcessError {}

impl From<StorageError> for ProcessError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<aisdk::Error> for ProcessError {
    fn from(value: aisdk::Error) -> Self {
        Self::Llm(value)
    }
}

impl From<TelegramClientError> for ProcessError {
    fn from(value: TelegramClientError) -> Self {
        Self::Telegram(value)
    }
}

#[derive(Debug)]
struct DraftStreamState {
    accumulated: String,
    last_draft_at: Instant,
    last_draft_text: String,
    formatted_chunks: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FollowUpActionsResult {
    actions: Vec<FollowUpActionResult>,
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Debug, Deserialize)]
struct FollowUpActionResult {
    key: Option<String>,
    prompt: Option<String>,
    url: Option<String>,
}

impl FollowUpActionsResult {
    fn log_actions(&self) {
        for action in &self.actions {
            match (&action.key, &action.prompt, &action.url) {
                (Some(key), Some(prompt), None) => {
                    log::debug!("follow up action key {key} maps to prompt {prompt}");
                }
                (None, None, Some(url)) => {
                    log::debug!("menu action opens url {url}");
                }
                (Some(_key), None, None) => {}
                (None, Some(_prompt), None) => {}
                (None, None, None) => {}
                (Some(_key), Some(_prompt), Some(_url)) => {}
                (Some(_key), None, Some(_url)) => {}
                (None, Some(_prompt), Some(_url)) => {}
            }
        }
    }

    fn into_keyboard(self) -> InlineKeyboardMarkup {
        let Self {
            actions: _actions,
            inline_keyboard,
        } = self;

        InlineKeyboardMarkup { inline_keyboard }
    }
}

impl DraftStreamState {
    fn new(now: Instant) -> Self {
        Self {
            accumulated: String::new(),
            last_draft_at: now,
            last_draft_text: String::new(),
            formatted_chunks: Vec::new(),
        }
    }

    fn push_text(&mut self, text: &str) {
        self.accumulated.push_str(text);
    }

    fn should_emit_draft(&self, now: Instant) -> bool {
        !self.accumulated.is_empty()
            && self.accumulated != self.last_draft_text
            && now.duration_since(self.last_draft_at) >= DRAFT_INTERVAL
    }

    fn format_current(&mut self) {
        self.formatted_chunks = markdown_to_telegram_html_chunks(&self.accumulated);
        self.last_draft_text = self.accumulated.clone();
    }

    fn ensure_current_formatting(&mut self) {
        if self.accumulated != self.last_draft_text {
            self.format_current();
        }
    }
}

pub async fn process<S>(
    update: Update,
    storage: &S,
    config: &Config,
    telegram: &Telegram,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    match update.tag {
        UpdateTag::Message { message } => process_message(message, storage, config, telegram).await,
        UpdateTag::CallbackQuery { callback_query } => {
            process_callback_query(callback_query, storage, config, telegram).await
        }
        UpdateTag::Unsupported(_value) => Ok(()),
    }
}

pub fn to_prompt(message: TelegramMessage) -> Messages {
    match message.tag {
        MessageTag::Photo {
            photo: _photo,
            caption: _caption,
        } => Vec::new(),
        MessageTag::Document {
            document: _document,
            caption: _caption,
        } => Vec::new(),
        MessageTag::Location { location } => vec![AiMessage::User(UserMessage::new(format!(
            "User sent GPS coordinates: latitude: {}, longitude: {}. Suggest useful menu options.",
            location.latitude, location.longitude
        )))],
        MessageTag::Text { text } => vec![AiMessage::User(UserMessage::new(text))],
        MessageTag::Unsupported(_value) => Vec::new(),
    }
}

async fn process_message<S>(
    message: TelegramMessage,
    storage: &S,
    config: &Config,
    telegram: &Telegram,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    let chat_id = message.chat.id;
    let draft_id = MessageDraftId::new(message.message_id)
        .ok_or(ProcessError::InvalidDraftId(message.message_id))?;
    let prompt = to_prompt(message);

    if prompt.is_empty() {
        return Ok(());
    }

    process_prompt(chat_id, draft_id, prompt, storage, config, telegram).await
}

async fn process_prompt<S>(
    chat_id: ChatId,
    draft_id: MessageDraftId,
    prompt: Messages,
    storage: &S,
    config: &Config,
    telegram: &Telegram,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    telegram
        .with_chat_action(chat_id, ChatAction::Typing, async {
            let history = append_prompt_messages(storage, chat_id, prompt)?;
            let input_count = history.len();

            let mut response = llm::stream_with_tool(history, config).await?;

            let draft_state =
                match consume_response_stream(telegram, chat_id, draft_id, &mut response).await {
                    Ok(draft_state) => draft_state,
                    Err(error) => return Err(error),
                };

            finish_response(
                storage,
                chat_id,
                telegram,
                &mut response,
                input_count,
                draft_state,
            )
            .await
        })
        .await
}

async fn consume_response_stream(
    telegram: &Telegram,
    chat_id: ChatId,
    draft_id: MessageDraftId,
    response: &mut StreamTextResponse,
) -> Result<DraftStreamState, ProcessError> {
    let mut draft_state = DraftStreamState::new(Instant::now());
    let mut stream_error = None;

    while let Some(chunk) = response.stream.next().await {
        match chunk {
            LanguageModelStreamChunkType::Failed(error) => {
                stream_error = Some(error);
            }
            LanguageModelStreamChunkType::Start => {}
            LanguageModelStreamChunkType::Text(text) => {
                draft_state.push_text(&text);

                if draft_state.should_emit_draft(Instant::now()) {
                    send_current_draft(telegram, chat_id, draft_id, &mut draft_state).await?;
                }
            }
            LanguageModelStreamChunkType::Reasoning(_text) => {}
            LanguageModelStreamChunkType::ToolCall(text) => {
                log::debug!("{}", text)
            }
            LanguageModelStreamChunkType::End(_assistant) => {}
            LanguageModelStreamChunkType::Incomplete(_message) => {}
            LanguageModelStreamChunkType::NotSupported(_message) => {}
        }
    }

    match stream_error {
        Some(error) => Err(ProcessError::StreamFailed(error)),
        None => Ok(draft_state),
    }
}

async fn finish_response<S>(
    storage: &S,
    chat_id: ChatId,
    telegram: &Telegram,
    response: &mut StreamTextResponse,
    input_count: usize,
    mut draft_state: DraftStreamState,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    draft_state.ensure_current_formatting();
    let response_messages = response.messages().await;
    let follow_up_actions =
        follow_up_actions_from_response_messages(&response_messages, input_count);
    append_response_messages(storage, chat_id, response_messages, input_count)?;
    let keyboard = match follow_up_actions {
        Some(actions) => Some(actions.into_keyboard()),
        None => None,
    };
    let last_sent_message_id =
        send_final_messages(telegram, chat_id, &draft_state.formatted_chunks).await?;
    log::debug!("last sent telegram message id: {last_sent_message_id:?}");

    match (last_sent_message_id, keyboard) {
        (Some(message_id), Some(keyboard)) => {
            match telegram
                .edit_message_reply_markup(chat_id, message_id, keyboard)
                .await
            {
                Ok(_message) => {}
                Err(error) if error.is_message_not_modified() => {
                    log::debug!("inline keyboard unchanged for message {message_id}");
                }
                Err(error) => return Err(ProcessError::Telegram(error)),
            }
        }
        (Some(_message_id), None) => {}
        (None, Some(_keyboard)) => {
            log::debug!("inline keyboard skipped because no final message was sent");
        }
        (None, None) => {}
    }

    Ok(())
}

async fn send_current_draft(
    telegram: &Telegram,
    chat_id: ChatId,
    draft_id: MessageDraftId,
    draft_state: &mut DraftStreamState,
) -> Result<(), ProcessError> {
    draft_state.format_current();
    draft_state.last_draft_at = Instant::now();

    match draft_state.formatted_chunks.first() {
        Some(text) => {
            send_message_draft_best_effort(telegram, chat_id, draft_id, text.clone()).await;
        }
        None => {}
    }

    Ok(())
}

async fn send_message_draft_best_effort(
    telegram: &Telegram,
    chat_id: ChatId,
    draft_id: MessageDraftId,
    text: String,
) {
    match telegram
        .send_message_draft(chat_id, draft_id, Some(text), None)
        .await
    {
        Ok(_sent) => {}
        Err(error) => {
            log::warn!("message draft update failed: {error}");
        }
    }
}

async fn send_final_messages(
    telegram: &Telegram,
    chat_id: ChatId,
    formatted_chunks: &[String],
) -> Result<Option<i64>, ProcessError> {
    let mut last_sent_message_id = None;

    for chunk in formatted_chunks {
        let message = telegram
            .send_message(chat_id, chunk.clone(), SendMessageOptions::Plain)
            .await?;
        last_sent_message_id = Some(message.message_id);
    }

    Ok(last_sent_message_id)
}

fn append_prompt_messages<S>(
    storage: &S,
    chat_id: ChatId,
    prompt: Messages,
) -> Result<Messages, StorageError>
where
    S: MessageStorage,
{
    prompt
        .into_iter()
        .try_fold(storage.messages(chat_id)?, |_messages, message| {
            storage.append_message(chat_id, message)
        })
}

fn append_response_messages<S>(
    storage: &S,
    chat_id: ChatId,
    messages: Messages,
    input_count: usize,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    messages
        .into_iter()
        .skip(input_count)
        .try_fold((), |(), message| {
            storage
                .append_message(chat_id, message)
                .map(|_messages| ())
                .map_err(ProcessError::Storage)
        })
}

fn follow_up_actions_from_response_messages(
    messages: &[AiMessage],
    input_count: usize,
) -> Option<FollowUpActionsResult> {
    let mut follow_up_actions = None;

    for message in messages.iter().skip(input_count) {
        match message {
            AiMessage::Tool(tool) => match &tool.output {
                Ok(output) if tool.tool.name == llm::FOLLOW_UP_ACTIONS_TOOL_NAME => match output {
                    serde_json::Value::String(text) => {
                        log::debug!("follow up actions tool output: {text}");
                        match serde_json::from_str::<FollowUpActionsResult>(text) {
                            Ok(parsed) => {
                                parsed.log_actions();
                                follow_up_actions = Some(parsed);
                            }
                            Err(error) => log::warn!("invalid follow up tool result: {error}"),
                        }
                    }
                    other => {
                        log::warn!("invalid follow up tool output type: {other:?}");
                    }
                },
                Ok(_output) => {}
                Err(error) if tool.tool.name == llm::FOLLOW_UP_ACTIONS_TOOL_NAME => {
                    log::warn!("follow up actions tool failed: {error}");
                }
                Err(_error) => {}
            },
            AiMessage::System(_system) => {}
            AiMessage::User(_user) => {}
            AiMessage::Assistant(_assistant) => {}
            AiMessage::Developer(_developer) => {}
        }
    }

    follow_up_actions
}

async fn process_callback_query<S>(
    callback_query: CallbackQuery,
    storage: &S,
    config: &Config,
    telegram: &Telegram,
) -> Result<(), ProcessError>
where
    S: MessageStorage,
{
    let callback_query_id = callback_query.id;

    match (callback_query.data, callback_query.message) {
        (Some(key), Some(message)) => {
            let draft_id = MessageDraftId::new(message.message_id)
                .ok_or(ProcessError::InvalidDraftId(message.message_id))?;

            telegram
                .answer_callback_query(callback_query_id, CallbackQueryAnswer::Empty)
                .await?;

            process_prompt(
                message.chat.id,
                draft_id,
                vec![AiMessage::User(UserMessage::new(selected_key_prompt(&key)))],
                storage,
                config,
                telegram,
            )
            .await
        }
        (Some(_prompt), None) => {
            telegram
                .answer_callback_query(
                    callback_query_id,
                    CallbackQueryAnswer::Alert {
                        text: String::from("I can't read that chat anymore."),
                    },
                )
                .await?;

            Ok(())
        }
        (None, Some(_message)) => {
            telegram
                .answer_callback_query(callback_query_id, CallbackQueryAnswer::Empty)
                .await?;

            Ok(())
        }
        (None, None) => {
            telegram
                .answer_callback_query(callback_query_id, CallbackQueryAnswer::Empty)
                .await?;

            Ok(())
        }
    }
}

fn selected_key_prompt(key: &str) -> String {
    format!(
        "The user tapped the menu button with key `{key}`. Use the previous follow_up_actions tool output in this conversation to find the matching prompt for that key, then answer that prompt. Call follow_up_actions exactly once for the new answer."
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DRAFT_INTERVAL, DraftStreamState, append_prompt_messages, append_response_messages,
        follow_up_actions_from_response_messages, selected_key_prompt, to_prompt,
    };
    use crate::{
        llm,
        storage::{InMemoryStorage, MessageStorage},
        telegram::{
            Chat, InlineKeyboardButton, InlineKeyboardMarkup, Location, Message as TelegramMessage,
            MessageTag,
        },
    };
    use aisdk::core::{
        AssistantMessage, Message as AiMessage, ToolCallInfo, ToolResultInfo,
        language_model::LanguageModelResponseContentType,
    };
    use serde_json::Value;
    use std::time::Duration;
    use tokio::time::Instant;

    #[test]
    fn text_message_becomes_user_prompt() {
        let prompt = to_prompt(TelegramMessage {
            message_id: 1,
            chat: Chat { id: 42 },
            tag: MessageTag::Text {
                text: String::from("hello"),
            },
        });

        assert_eq!(prompt.len(), 1);
        match &prompt[0] {
            AiMessage::System(_system) => panic!("expected user message"),
            AiMessage::User(user) => assert_eq!(user.content, "hello"),
            AiMessage::Assistant(_assistant) => panic!("expected user message"),
            AiMessage::Tool(_tool) => panic!("expected user message"),
            AiMessage::Developer(_developer) => panic!("expected user message"),
        }
    }

    #[test]
    fn location_message_becomes_user_prompt() {
        let prompt = to_prompt(TelegramMessage {
            message_id: 1,
            chat: Chat { id: 42 },
            tag: MessageTag::Location {
                location: Location {
                    latitude: -31.5375,
                    longitude: -68.5364,
                },
            },
        });

        assert_eq!(prompt.len(), 1);
        match &prompt[0] {
            AiMessage::System(_system) => panic!("expected user message"),
            AiMessage::User(user) => {
                assert_eq!(
                    user.content,
                    "User sent GPS coordinates: latitude: -31.5375, longitude: -68.5364. Suggest useful menu options."
                );
            }
            AiMessage::Assistant(_assistant) => panic!("expected user message"),
            AiMessage::Tool(_tool) => panic!("expected user message"),
            AiMessage::Developer(_developer) => panic!("expected user message"),
        }
    }

    #[test]
    fn prompt_storage_keeps_user_messages_only() -> Result<(), crate::storage::StorageError> {
        let storage = InMemoryStorage::new();
        let chat_id = 42;
        let prompt = to_prompt(TelegramMessage {
            message_id: 1,
            chat: Chat { id: chat_id },
            tag: MessageTag::Location {
                location: Location {
                    latitude: 1.0,
                    longitude: 2.0,
                },
            },
        });

        append_prompt_messages(&storage, chat_id, prompt)?;

        let messages = storage.messages(chat_id)?;
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            AiMessage::System(_system) => panic!("expected user message"),
            AiMessage::User(user) => assert_eq!(
                user.content,
                "User sent GPS coordinates: latitude: 1, longitude: 2. Suggest useful menu options."
            ),
            AiMessage::Assistant(_assistant) => panic!("expected user message"),
            AiMessage::Tool(_tool) => panic!("expected user message"),
            AiMessage::Developer(_developer) => panic!("expected user message"),
        }

        Ok(())
    }

    #[test]
    fn prompt_storage_keeps_existing_tool_history() -> Result<(), crate::storage::StorageError> {
        let storage = InMemoryStorage::new();
        let chat_id = 42;
        storage.append_message(
            chat_id,
            AiMessage::User(aisdk::core::UserMessage::new("old")),
        )?;
        storage.append_message(
            chat_id,
            AiMessage::Assistant(AssistantMessage::new(
                LanguageModelResponseContentType::ToolCall(ToolCallInfo::new(
                    llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                )),
                None,
            )),
        )?;
        storage.append_message(
            chat_id,
            AiMessage::Tool(tool_result(
                llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                r#"{"actions":[{"key":"old","label":"Old","prompt":"Old prompt"}],"inline_keyboard":[[{"text":"Old","callback_data":"old"}]]}"#,
            )),
        )?;

        let history = append_prompt_messages(
            &storage,
            chat_id,
            vec![AiMessage::User(aisdk::core::UserMessage::new("new"))],
        )?;

        assert_eq!(history.len(), 4);
        assert!(matches!(&history[0], AiMessage::User(_user)));
        assert!(matches!(&history[1], AiMessage::Assistant(_assistant)));
        assert!(matches!(&history[2], AiMessage::Tool(_tool)));
        assert!(matches!(&history[3], AiMessage::User(_user)));
        assert_eq!(storage.messages(chat_id)?.len(), 4);

        Ok(())
    }

    #[test]
    fn response_storage_keeps_tool_messages() {
        let storage = InMemoryStorage::new();
        let chat_id = 42;
        let response_messages = vec![
            AiMessage::User(aisdk::core::UserMessage::new("hello")),
            AiMessage::Assistant(AssistantMessage::new(
                LanguageModelResponseContentType::ToolCall(ToolCallInfo::new(
                    llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                )),
                None,
            )),
            AiMessage::Tool(tool_result(
                llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                r#"{"actions":[{"key":"next","label":"Next","prompt":"Next prompt"}],"inline_keyboard":[[{"text":"Next","callback_data":"next"}]]}"#,
            )),
            AiMessage::Assistant(AssistantMessage::new(
                LanguageModelResponseContentType::Text(String::from("final answer")),
                None,
            )),
        ];

        append_response_messages(&storage, chat_id, response_messages, 1)
            .expect("response messages should append");

        let messages = storage.messages(chat_id).expect("messages should load");
        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0], AiMessage::Assistant(_assistant)));
        assert!(matches!(&messages[1], AiMessage::Tool(_tool)));
        match &messages[2] {
            AiMessage::Assistant(assistant) => match &assistant.content {
                LanguageModelResponseContentType::Text(text) => assert_eq!(text, "final answer"),
                LanguageModelResponseContentType::ToolCall(_tool) => panic!("expected text"),
                LanguageModelResponseContentType::Reasoning {
                    content: _content,
                    extensions: _extensions,
                } => panic!("expected text"),
                LanguageModelResponseContentType::NotSupported(_message) => panic!("expected text"),
            },
            AiMessage::System(_system) => panic!("expected assistant message"),
            AiMessage::User(_user) => panic!("expected assistant message"),
            AiMessage::Tool(_tool) => panic!("expected assistant message"),
            AiMessage::Developer(_developer) => panic!("expected assistant message"),
        };
    }

    #[test]
    fn selected_key_prompt_asks_model_to_resolve_key_from_history() {
        let prompt = selected_key_prompt("explain:concepts");

        assert!(prompt.contains("explain:concepts"));
        assert!(prompt.contains("previous follow_up_actions tool output"));
    }

    #[test]
    fn draft_state_emits_at_fixed_interval_when_text_changes() {
        let start = Instant::now();
        let mut state = DraftStreamState::new(start);

        state.push_text("hello");

        assert!(!state.should_emit_draft(start + Duration::from_millis(499)));
        assert!(state.should_emit_draft(start + DRAFT_INTERVAL));

        state.format_current();
        state.last_draft_at = start + DRAFT_INTERVAL;

        assert!(!state.should_emit_draft(start + DRAFT_INTERVAL + DRAFT_INTERVAL));

        state.push_text(" world");

        assert!(state.should_emit_draft(start + DRAFT_INTERVAL + DRAFT_INTERVAL));
    }

    #[test]
    fn draft_state_reuses_current_formatted_chunks() {
        let start = Instant::now();
        let mut state = DraftStreamState::new(start);

        state.push_text("**hello**");
        state.format_current();

        let formatted = state.formatted_chunks.clone();
        state.ensure_current_formatting();

        assert_eq!(state.formatted_chunks, formatted);

        state.push_text(" world");
        state.ensure_current_formatting();

        assert_ne!(state.formatted_chunks, formatted);
        assert_eq!(
            state.formatted_chunks,
            vec![String::from("<b>hello</b> world")]
        );
    }

    #[test]
    fn extracts_latest_follow_up_actions_tool_result() {
        let messages = vec![
            AiMessage::User(aisdk::core::UserMessage::new("hello")),
            AiMessage::Tool(tool_result(
                llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                r#"{"actions":[{"key":"old","label":"Old","prompt":"Old prompt"}],"inline_keyboard":[[{"text":"Old","callback_data":"old"}]]}"#,
            )),
            AiMessage::Tool(tool_result(
                llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
                r#"{"actions":[{"key":"new","label":"New","prompt":"New prompt"},{"key":"details","label":"Details","prompt":"Details prompt"}],"inline_keyboard":[[{"text":"New","callback_data":"new"}],[{"text":"Details","callback_data":"details"}]]}"#,
            )),
        ];

        let actions = follow_up_actions_from_response_messages(&messages, 1)
            .expect("follow up actions should be extracted");

        assert_eq!(actions.actions.len(), 2);
        assert_eq!(actions.actions[0].key.as_deref(), Some("new"));
        assert_eq!(actions.actions[0].prompt.as_deref(), Some("New prompt"));
        assert_eq!(
            actions.into_keyboard(),
            InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![InlineKeyboardButton::CallbackData {
                        text: String::from("New"),
                        callback_data: String::from("new"),
                    }],
                    vec![InlineKeyboardButton::CallbackData {
                        text: String::from("Details"),
                        callback_data: String::from("details"),
                    }]
                ]
            }
        );
    }

    #[test]
    fn ignores_unrelated_tool_results() {
        let messages = vec![AiMessage::Tool(tool_result(
            "other_tool",
            r#"{"actions":[{"key":"other","label":"Other","prompt":"Other prompt"}],"inline_keyboard":[[{"text":"Other","callback_data":"other"}]]}"#,
        ))];

        assert!(follow_up_actions_from_response_messages(&messages, 0).is_none());
    }

    #[test]
    fn ignores_invalid_inline_keyboard_tool_result() {
        let messages = vec![AiMessage::Tool(tool_result(
            llm::FOLLOW_UP_ACTIONS_TOOL_NAME,
            "not json",
        ))];

        assert!(follow_up_actions_from_response_messages(&messages, 0).is_none());
    }

    fn tool_result(name: &str, output: &str) -> ToolResultInfo {
        let mut result = ToolResultInfo::new(name);
        result.output(Value::String(output.to_owned()));
        result
    }
}
