type TelegramSetWebhookResponse =
  | {
      ok: true;
      result: true;
      description?: string;
    }
  | {
      ok: false;
      description?: string;
      error_code?: number;
    };

const requiredEnv = (name: string): string => {
  const value = Bun.env[name];

  if (value === undefined || value.length === 0) {
    throw new Error(`${name} environment variable is required`);
  }

  return value;
};

const telegramToken = requiredEnv("TELEGRAM_BOT_TOKEN");
const webhookSecret = requiredEnv("WEBHOOK_SECRET");
const vercelProductionUrl = requiredEnv("VERCEL_PROJECT_PRODUCTION_URL");
const webhookUrl = `https://${vercelProductionUrl}/api/telegram`;

const body = new URLSearchParams({
  url: webhookUrl,
  secret_token: webhookSecret,
  allowed_updates: JSON.stringify(["message", "callback_query"]),
});

const response = await fetch(
  `https://api.telegram.org/bot${telegramToken}/setWebhook`,
  {
    method: "POST",
    body,
  },
);

if (!response.ok) {
  throw new Error(`setWebhook HTTP ${response.status}`);
}

const payload = (await response.json()) as TelegramSetWebhookResponse;

if (payload.ok !== true) {
  throw new Error(payload.description ?? "setWebhook failed");
}

console.log(`Webhook set to ${webhookUrl}`);

export {};
