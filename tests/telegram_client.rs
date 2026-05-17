use ollagram::telegram::{
    GetUpdatesOptions, InlineKeyboardButton, InlineKeyboardButtonStyle, InlineKeyboardMarkup,
    MessageDraftId, SendMessageOptions, Telegram,
};
use std::{env, error::Error, num::NonZeroI64};

fn telegram() -> Result<Telegram, env::VarError> {
    env::var("TELEGRAM_BOT_TOKEN").map(Telegram::new)
}

fn chat_id() -> Result<i64, Box<dyn Error>> {
    env::var("TELEGRAM_TEST_CHAT_ID")?
        .parse()
        .map_err(Into::into)
}

fn draft_id() -> MessageDraftId {
    NonZeroI64::new(1).expect("test draft id must be non-zero")
}

#[tokio::test]
#[ignore = "requires TELEGRAM_BOT_TOKEN and calls the real Telegram API"]
async fn gets_updates_with_real_bot_credentials() -> Result<(), Box<dyn Error>> {
    let bot = telegram()?;

    let _updates = bot
        .get_updates(GetUpdatesOptions::Timeout {
            offset: None,
            timeout: 0,
        })
        .await?;

    Ok(())
}

#[tokio::test]
#[ignore = "requires TELEGRAM_BOT_TOKEN, TELEGRAM_TEST_CHAT_ID, and sends real Telegram messages"]
async fn sends_message_draft_and_final_message_with_real_bot_credentials()
-> Result<(), Box<dyn Error>> {
    let bot = telegram()?;
    let chat_id = chat_id()?;

    bot.send_message_draft(
        chat_id,
        draft_id(),
        Some(String::from("Integration test draft")),
        None,
    )
    .await?;

    let message = bot
        .send_message(
            chat_id,
            String::from("Integration test final message"),
            SendMessageOptions::Plain,
        )
        .await?;

    assert!(message.message_id > 0);

    Ok(())
}

#[tokio::test]
#[ignore = "requires TELEGRAM_BOT_TOKEN, TELEGRAM_TEST_CHAT_ID, and sends a real Telegram message"]
async fn sends_message_with_inline_keyboard_with_real_bot_credentials() -> Result<(), Box<dyn Error>>
{
    let bot = telegram()?;
    let chat_id = chat_id()?;

    let message = bot
        .send_message(
            chat_id,
            String::from("Integration test keyboard"),
            SendMessageOptions::InlineKeyboard(InlineKeyboardMarkup {
                inline_keyboard: vec![vec![InlineKeyboardButton::Url {
                    text: String::from("Telegram Bot API"),
                    url: String::from("https://core.telegram.org/bots/api"),
                }]],
            }),
        )
        .await?;

    assert!(message.message_id > 0);

    Ok(())
}

#[tokio::test]
#[ignore = "requires TELEGRAM_BOT_TOKEN, TELEGRAM_TEST_CHAT_ID, and sends a real Telegram message"]
async fn sends_message_with_callback_data_keyboard_with_real_bot_credentials()
-> Result<(), Box<dyn Error>> {
    let bot = telegram()?;
    let chat_id = chat_id()?;

    let message = bot
        .send_message(
            chat_id,
            String::from("Integration test callback grid"),
            SendMessageOptions::InlineKeyboard(InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        InlineKeyboardButton::CallbackDataStyled {
                            text: String::from("🚀 Launch"),
                            callback_data: String::from("integration:launch"),
                            style: InlineKeyboardButtonStyle::Primary,
                        },
                        InlineKeyboardButton::CallbackDataStyled {
                            text: String::from("✅ Confirm"),
                            callback_data: String::from("integration:confirm"),
                            style: InlineKeyboardButtonStyle::Success,
                        },
                    ],
                    vec![
                        InlineKeyboardButton::CallbackData {
                            text: String::from("💬 Details"),
                            callback_data: String::from("integration:details"),
                        },
                        InlineKeyboardButton::CallbackDataStyled {
                            text: String::from("⚠️ Cancel"),
                            callback_data: String::from("integration:cancel"),
                            style: InlineKeyboardButtonStyle::Danger,
                        },
                    ],
                ],
            }),
        )
        .await?;

    assert!(message.message_id > 0);

    Ok(())
}
