# Ollagram

Telegram bot written in Rust and deployed as a Vercel Rust function.

[![Deploy with Vercel](https://vercel.com/button)](https://vercel.com/new/clone?repository-url=https%3A%2F%2Fgithub.com%2Fricardocasares%2Follagram&env=TELEGRAM_BOT_TOKEN%2CWEBHOOK_SECRET%2COPENAI_API_KEY%2COPENAI_URL%2COPENAI_MODEL%2CSYSTEM_PROMPT%2CSYSTEM_PROMPT_APPEND&envDefaults=%7B%22TELEGRAM_BOT_TOKEN%22%3A%22_your_telegram_bot_token_%22%2C%22WEBHOOK_SECRET%22%3A%22_random_secret_%22%2C%22OPENAI_API_KEY%22%3A%22_your_api_key_%22%2C%22OPENAI_URL%22%3A%22https%3A%2F%2Follama.com%2Fv1%22%2C%22OPENAI_MODEL%22%3A%22gpt-oss%3A120b-cloud%22%2C%22SYSTEM_PROMPT%22%3A%22%22%2C%22SYSTEM_PROMPT_APPEND%22%3A%22%22%7D&project-name=ollagrambot&repository-name=ollagrambot)

## Requirements

- Rust stable
- Bun 1.3.14
- Telegram bot token from BotFather
- OpenAI-compatible API key

## Environment

Required:

- `TELEGRAM_BOT_TOKEN`
- `WEBHOOK_SECRET`
- `OPENAI_API_KEY`
- `OPENAI_URL`
- `OPENAI_MODEL`

Optional:

- `SYSTEM_PROMPT`: full replacement for the built-in system prompt.
- `SYSTEM_PROMPT_APPEND`: extra instructions appended to the built-in system prompt.
- `RUST_LOG`: log filter for Rust logs. Defaults to `info`; use `ollagram=debug` temporarily for webhook diagnostics.

Local defaults for `OPENAI_URL` and `OPENAI_MODEL` live in `.cargo/config.toml`. Local secrets can be placed in `.cargo/secrets.toml`, which is ignored by Git and optional for Cargo.

Example `.cargo/secrets.toml`:

```toml
[env]
TELEGRAM_BOT_TOKEN = "123456:telegram-token"
WEBHOOK_SECRET = "random-secret"
OPENAI_API_KEY = "api-key"
SYSTEM_PROMPT = "You are a concise Telegram assistant."
SYSTEM_PROMPT_APPEND = "Answer in Rioplatense Spanish."
```

## Run Locally

Run the polling bot:

```bash
cargo dev
```

Run tests:

```bash
cargo test --all-targets
```

Check formatting:

```bash
cargo fmt --check
```

## Deploy

Deploy to Vercel with the button above or connect the repository manually.

Vercel builds the Rust API function from `api/telegram.rs`. The `build` script runs `scripts/webhook.ts`, which calls Telegram `setWebhook` for:

```text
https://$VERCEL_PROJECT_PRODUCTION_URL/api/telegram
```

`VERCEL_PROJECT_PRODUCTION_URL` is provided by Vercel. The webhook installer requires Bun and the env vars listed above.
