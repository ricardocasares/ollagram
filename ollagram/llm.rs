use crate::config::Config;
use crate::telegram::InlineKeyboardButton;
use aisdk::core::{
    DynamicModel, LanguageModelRequest, Messages, StreamTextResponse,
    tools::{Tool, ToolExecute},
};
use aisdk::providers::OpenAICompatible;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const FOLLOW_UP_ACTIONS_TOOL_NAME: &str = "follow_up_actions";
const FOLLOW_UP_ACTION_LIMIT: usize = 3;
const CALLBACK_KEY_LIMIT: usize = 64;
const SYSTEM_PROMPT: &str = concat!(
    "Keep responses short unless explicitly told to expand. ",
    "Avoid using markdown tables and '---'. ",
    "You are a helpful assistant with access to tools. ",
    "For every answer, call the follow_up_actions tool exactly once before the final response. ",
    "Use it to structure the answer into one to three useful menu buttons for navigation, refinement, or next steps. ",
    "Each button must have a label and either a url field or both key and prompt fields. ",
    "You can include emojis in labels. ",
    "Key values are callback data and must be stable, concise, and 1-64 UTF-8 bytes. ",
    "Use namespaced keys like explain:concepts. ",
    "The prompt can be longer than the key and should express the full intent behind the menu item. ",
    "Write each prompt in the same language the user is using with you. ",
    "Send URLs through follow_up_actions url buttons, not in the final response text. ",
    "Use url buttons only for real known URLs. Do not invent YouTube links, website links, map links, document links, or any other direct URLs. ",
    "When you are not certain of an exact real URL, use a YouTube, Spotify, or Google search URL instead. ",
    "Do not repeat the menu buttons in the final response. ",
);

#[derive(Debug, Deserialize)]
struct FollowUpActionsInput {
    actions: Vec<FollowUpActionInput>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FollowUpActionInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Debug, Serialize)]
struct FollowUpActionsOutput {
    actions: Vec<FollowUpActionInput>,
    inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

impl JsonSchema for FollowUpActionsInput {
    fn schema_name() -> Cow<'static, str> {
        "FollowUpActionsInput".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::FollowUpActionsInput").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "required": ["actions"],
            "properties": {
                "actions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": FOLLOW_UP_ACTION_LIMIT,
                    "description": "One to three menu buttons that organize useful navigation, refinements, or next steps for the answer",
                    "items": generator.subschema_for::<FollowUpActionInput>()
                }
            }
        })
    }
}

impl JsonSchema for FollowUpActionInput {
    fn schema_name() -> Cow<'static, str> {
        "FollowUpActionInput".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::FollowUpActionInput").into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "required": ["label"],
            "properties": {
                "key": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": CALLBACK_KEY_LIMIT,
                    "description": "Stable callback key for the menu item, such as explain:concepts"
                },
                "label": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Short menu button label shown to the user"
                },
                "prompt": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Full prompt to run when the user selects this menu item"
                },
                "url": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Absolute URL to open when the user selects this menu item"
                }
            }
        })
    }
}

pub async fn stream_with_tool(
    messages: Messages,
    config: &Config,
) -> aisdk::Result<StreamTextResponse> {
    stream_with_tool_request(messages, config).await
}

async fn stream_with_tool_request(
    messages: Messages,
    config: &Config,
) -> aisdk::Result<StreamTextResponse> {
    let model = build_model(config)?;

    LanguageModelRequest::builder()
        .model(model)
        .system(system_prompt(config))
        .messages(messages)
        .with_tool(follow_up_actions())
        .build()
        .stream_text()
        .await
}

fn build_model(config: &Config) -> aisdk::Result<OpenAICompatible<DynamicModel>> {
    OpenAICompatible::<DynamicModel>::builder()
        .base_url(config.openai_url.clone())
        .api_key(config.openai_key.clone())
        .model_name(config.openai_model.clone())
        .build()
}

fn system_prompt(config: &Config) -> Cow<'static, str> {
    match (&config.system_prompt, &config.system_prompt_append) {
        (Some(system_prompt), Some(append)) => Cow::Owned(format!("{system_prompt}\n\n{append}")),
        (Some(system_prompt), None) => Cow::Owned(system_prompt.clone()),
        (None, Some(append)) => Cow::Owned(format!("{SYSTEM_PROMPT}\n\n{append}")),
        (None, None) => Cow::Borrowed(SYSTEM_PROMPT),
    }
}

fn follow_up_actions() -> Tool {
    Tool::builder()
        .name(FOLLOW_UP_ACTIONS_TOOL_NAME)
        .description(concat!(
            "Build one to three menu buttons that structure the final answer and give the user useful navigation paths, refinements, or next steps. ",
            "The input object must contain an actions array. ",
            "Each action must contain a label and either a url field or both key and prompt fields. ",
            "The key is callback data and must be 1-64 UTF-8 bytes. ",
            "The label is the button text. ",
            "The prompt is returned in the tool output so later callbacks can resolve the key from message history and continue with the selected menu item. ",
            "Write each prompt in the same language the user is using with you. ",
            "Send URLs through url actions, not in final response text. ",
            "The url is used only for real known URLs. Do not invent direct URLs. Use YouTube, Spotify, or Google search URLs when unsure."
        ))
        .input_schema(schemars::schema_for!(FollowUpActionsInput))
        .execute(ToolExecute::new(Box::new(execute_follow_up_actions)))
        .build()
        .expect("follow up actions tool should build")
}

fn execute_follow_up_actions(input: serde_json::Value) -> Result<String, String> {
    log::debug!("follow up actions tool raw input: {input}");
    let input = serde_json::from_value::<FollowUpActionsInput>(input).map_err(|error| {
        let message = format!("invalid follow up actions tool input: {error}");
        log::warn!("{message}");
        message
    })?;
    log::debug!("follow up actions tool input: {input:?}");

    let actions = validate_actions(input.actions).map_err(|error| {
        log::warn!("invalid follow up actions tool input: {error}");
        error
    })?;
    let inline_keyboard = actions.iter().map(action_to_button_row).collect();

    let keyboard = FollowUpActionsOutput {
        actions,
        inline_keyboard,
    };

    serde_json::to_string(&keyboard)
        .map_err(|error| format!("invalid inline keyboard json: {error}"))
        .inspect(|output| log::debug!("follow up actions tool output: {output}"))
}

fn validate_actions(actions: Vec<FollowUpActionInput>) -> Result<Vec<FollowUpActionInput>, String> {
    match actions.len() {
        1..=FOLLOW_UP_ACTION_LIMIT => {
            for action in &actions {
                validate_label(&action.label)?;
                validate_action_target(action)?;
            }

            Ok(actions)
        }
        0 => Err(String::from(
            "follow up actions must include at least one action",
        )),
        length => Err(format!(
            "follow up actions must include at most {FOLLOW_UP_ACTION_LIMIT} actions, got {length}"
        )),
    }
}

fn action_to_button_row(action: &FollowUpActionInput) -> Vec<InlineKeyboardButton> {
    match (&action.key, &action.prompt, &action.url) {
        (Some(key), Some(_prompt), None) => vec![InlineKeyboardButton::CallbackData {
            text: action.label.clone(),
            callback_data: key.clone(),
        }],
        (None, None, Some(url)) => vec![InlineKeyboardButton::Url {
            text: action.label.clone(),
            url: url.clone(),
        }],
        (Some(_key), None, None) => Vec::new(),
        (None, Some(_prompt), None) => Vec::new(),
        (None, None, None) => Vec::new(),
        (Some(_key), Some(_prompt), Some(_url)) => Vec::new(),
        (Some(_key), None, Some(_url)) => Vec::new(),
        (None, Some(_prompt), Some(_url)) => Vec::new(),
    }
}

fn validate_action_target(action: &FollowUpActionInput) -> Result<(), String> {
    match (&action.key, &action.prompt, &action.url) {
        (Some(key), Some(prompt), None) => {
            validate_callback_key(key)?;
            validate_prompt(prompt)
        }
        (None, None, Some(url)) => validate_url(url),
        (Some(_key), None, None) => Err(String::from(
            "menu prompt action must include both key and prompt",
        )),
        (None, Some(_prompt), None) => Err(String::from(
            "menu prompt action must include both key and prompt",
        )),
        (None, None, None) => Err(String::from(
            "menu action must include either url or key and prompt",
        )),
        (Some(_key), Some(_prompt), Some(_url)) => Err(String::from(
            "menu action must include either url or key and prompt, not both",
        )),
        (Some(_key), None, Some(_url)) => Err(String::from(
            "menu action must include either url or key and prompt, not both",
        )),
        (None, Some(_prompt), Some(_url)) => Err(String::from(
            "menu action must include either url or key and prompt, not both",
        )),
    }
}

fn validate_label(label: &str) -> Result<(), String> {
    match label.trim().is_empty() {
        false => Ok(()),
        true => Err(String::from("follow up button label must not be empty")),
    }
}

fn validate_callback_key(key: &str) -> Result<(), String> {
    match key.as_bytes().len() {
        1..=CALLBACK_KEY_LIMIT => Ok(()),
        0 => Err(String::from("callback key must not be empty")),
        length => Err(format!(
            "callback key must be at most {CALLBACK_KEY_LIMIT} bytes, got {length}"
        )),
    }
}

fn validate_prompt(prompt: &str) -> Result<(), String> {
    match prompt.trim().is_empty() {
        false => Ok(()),
        true => Err(String::from("follow up prompt must not be empty")),
    }
}

fn validate_url(url: &str) -> Result<(), String> {
    match reqwest::Url::parse(url) {
        Ok(_url) if !url.trim().is_empty() => Ok(()),
        Ok(_url) => Err(String::from("menu url must not be empty")),
        Err(error) => Err(format!("menu url must be absolute and valid: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{FOLLOW_UP_ACTIONS_TOOL_NAME, SYSTEM_PROMPT, follow_up_actions, system_prompt};
    use crate::config::Config;
    use serde_json::json;

    fn config(system_prompt: Option<String>, system_prompt_append: Option<String>) -> Config {
        Config {
            telegram_token: String::from("telegram-token"),
            openai_url: String::from("https://example.com/v1"),
            openai_key: String::from("openai-key"),
            webhook_secret: String::from("webhook-secret"),
            openai_model: String::from("model"),
            system_prompt,
            system_prompt_append,
        }
    }

    #[test]
    fn system_prompt_uses_default_without_append() {
        assert_eq!(system_prompt(&config(None, None)).as_ref(), SYSTEM_PROMPT);
        assert!(SYSTEM_PROMPT.contains("same language the user is using"));
    }

    #[test]
    fn system_prompt_appends_custom_text() {
        let prompt = system_prompt(&config(
            None,
            Some(String::from("Answer in Rioplatense Spanish.")),
        ));

        assert!(prompt.starts_with(SYSTEM_PROMPT));
        assert!(prompt.ends_with("Answer in Rioplatense Spanish."));
    }

    #[test]
    fn system_prompt_replaces_default() {
        let prompt = system_prompt(&config(Some(String::from("Custom system prompt.")), None));

        assert_eq!(prompt.as_ref(), "Custom system prompt.");
    }

    #[test]
    fn system_prompt_appends_to_replacement() {
        let prompt = system_prompt(&config(
            Some(String::from("Custom system prompt.")),
            Some(String::from("Extra instruction.")),
        ));

        assert_eq!(
            prompt.as_ref(),
            "Custom system prompt.\n\nExtra instruction."
        );
    }

    #[test]
    fn follow_up_actions_tool_uses_ollama_compatible_schema() {
        let tool = follow_up_actions();
        let schema = tool.input_schema.to_value();

        assert_eq!(tool.name, FOLLOW_UP_ACTIONS_TOOL_NAME);
        assert!(!tool.description.contains("Telegram"));
        assert!(
            tool.description
                .contains("same language the user is using")
        );
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["actions"]));
        assert_eq!(schema["properties"]["actions"]["type"], json!("array"));
        assert_eq!(schema["properties"]["actions"]["minItems"], json!(1));
        assert_eq!(schema["properties"]["actions"]["maxItems"], json!(3));
        assert_eq!(
            schema["properties"]["actions"]["items"]["$ref"],
            json!("#/$defs/FollowUpActionInput")
        );
        assert_eq!(
            schema["$defs"]["FollowUpActionInput"]["required"],
            json!(["label"])
        );
        assert_eq!(
            schema["$defs"]["FollowUpActionInput"]["properties"]["key"]["maxLength"],
            json!(64)
        );
        assert_eq!(
            schema["$defs"]["FollowUpActionInput"]["properties"]["url"]["type"],
            json!("string")
        );
    }

    #[test]
    fn inline_keyboard_tool_builds_prompt_buttons() {
        let result = follow_up_actions()
            .execute
            .call(json!({
                "actions": [
                    {
                        "key": "summarize:this",
                        "label": "Summarize",
                        "prompt": "Summarize this"
                    },
                    {
                        "key": "explain:concepts",
                        "label": "Explain concepts",
                        "prompt": "Explain Elm language concepts"
                    }
                ]
            }))
            .expect("keyboard should be valid");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).expect("result should be json"),
            json!({
                "actions": [
                    {
                        "key": "summarize:this",
                        "label": "Summarize",
                        "prompt": "Summarize this"
                    },
                    {
                        "key": "explain:concepts",
                        "label": "Explain concepts",
                        "prompt": "Explain Elm language concepts"
                    }
                ],
                "inline_keyboard": [
                    [
                        {
                            "text": "Summarize",
                            "callback_data": "summarize:this"
                        }
                    ],
                    [
                        {
                            "text": "Explain concepts",
                            "callback_data": "explain:concepts"
                        }
                    ]
                ]
            })
        );
    }

    #[test]
    fn inline_keyboard_tool_builds_url_buttons() {
        let result = follow_up_actions()
            .execute
            .call(json!({
                "actions": [
                    {
                        "label": "Open docs",
                        "url": "https://core.telegram.org/bots/api"
                    },
                    {
                        "key": "explain:concepts",
                        "label": "Explain concepts",
                        "prompt": "Explain Elm language concepts"
                    }
                ]
            }))
            .expect("keyboard should be valid");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).expect("result should be json"),
            json!({
                "actions": [
                    {
                        "label": "Open docs",
                        "url": "https://core.telegram.org/bots/api"
                    },
                    {
                        "key": "explain:concepts",
                        "label": "Explain concepts",
                        "prompt": "Explain Elm language concepts"
                    }
                ],
                "inline_keyboard": [
                    [
                        {
                            "text": "Open docs",
                            "url": "https://core.telegram.org/bots/api"
                        }
                    ],
                    [
                        {
                            "text": "Explain concepts",
                            "callback_data": "explain:concepts"
                        }
                    ]
                ]
            })
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_old_actions_shape() {
        let result = follow_up_actions().execute.call(json!({
            "key": "summarize:this",
            "label": "Summarize",
            "prompt": "Summarize this"
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("missing field `actions`"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_empty_actions() {
        let result = follow_up_actions().execute.call(json!({}));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("missing field `actions`"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_more_than_three_actions() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [
                { "key": "one", "label": "One", "prompt": "One" },
                { "key": "two", "label": "Two", "prompt": "Two" },
                { "key": "three", "label": "Three", "prompt": "Three" },
                { "key": "four", "label": "Four", "prompt": "Four" }
            ]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("at most 3 actions")));
    }

    #[test]
    fn inline_keyboard_tool_rejects_missing_key() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "label": "Details", "prompt": "Details" }]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("both key and prompt")));
    }

    #[test]
    fn inline_keyboard_tool_rejects_missing_label() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "key": "details", "prompt": "Details" }]
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("missing field `label`"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_missing_prompt() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "key": "details", "label": "Details" }]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("both key and prompt")));
    }

    #[test]
    fn inline_keyboard_tool_rejects_action_with_url_and_prompt_target() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{
                "key": "details",
                "label": "Details",
                "prompt": "Details",
                "url": "https://example.com"
            }]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("not both")));
    }

    #[test]
    fn inline_keyboard_tool_rejects_invalid_url() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{
                "label": "Docs",
                "url": "/relative"
            }]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("absolute and valid")));
    }

    #[test]
    fn inline_keyboard_tool_rejects_empty_label() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "key": "details", "label": " ", "prompt": "Details" }]
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("label must not be empty"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_empty_key() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "key": "", "label": "Empty", "prompt": "Details" }]
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("key must not be empty"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_key_longer_than_sixty_four_bytes() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{
                "key": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "label": "Long",
                "prompt": "Details"
            }]
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("key must be at most 64 bytes"))
        );
    }

    #[test]
    fn inline_keyboard_tool_rejects_empty_prompt() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{ "key": "empty", "label": "Empty", "prompt": "" }]
        }));

        assert!(
            matches!(result, Err(error) if error.to_string().contains("prompt must not be empty"))
        );
    }

    #[test]
    fn inline_keyboard_tool_accepts_prompt_longer_than_sixty_four_bytes() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{
                "key": "long",
                "label": "Long",
                "prompt": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }]
        }));

        assert!(result.is_ok());
    }

    #[test]
    fn inline_keyboard_tool_counts_key_utf8_bytes() {
        let result = follow_up_actions().execute.call(json!({
            "actions": [{
                "key": "🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂",
                "label": "Long",
                "prompt": "Details"
            }]
        }));

        assert!(matches!(result, Err(error) if error.to_string().contains("got 68")));
    }
}
