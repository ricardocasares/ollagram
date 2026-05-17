use ollagram::webhook::handler;
use vercel_runtime::{Error, run, service_fn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = pretty_env_logger::formatted_timed_builder()
        .parse_env(pretty_env_logger::env_logger::Env::default().default_filter_or("info"))
        .try_init();
    let service = service_fn(handler);
    run(service).await
}
