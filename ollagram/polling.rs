use ::ollagram::{
    config, ollagram,
    storage::InMemoryStorage,
    telegram::{GetUpdatesOptions, Telegram},
};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = pretty_env_logger::formatted_timed_builder()
        .parse_env(pretty_env_logger::env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let cfg = config::from_env()?;
    let bot = Telegram::new(cfg.telegram_token.clone());
    let storage = InMemoryStorage::new();

    log::info!("polling started");

    let mut offset = None;
    loop {
        let updates = bot
            .get_updates(GetUpdatesOptions::DefaultTimeout { offset })
            .await?;
        log::debug!("{:?}", updates);

        for update in updates {
            log::info!("processing update {}", update.update_id);
            offset = Some(update.update_id + 1);
            match ollagram::process(update, &storage, &cfg, &bot).await {
                Ok(()) => {}
                Err(error) => log::error!("update processing failed: {error}"),
            }
        }
    }
}
