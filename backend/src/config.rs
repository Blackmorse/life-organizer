use std::{env, net::SocketAddr};

pub struct Config {
    pub database_url: String,
    pub telegram_bot_token: String,
    pub bind_address: SocketAddr,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|error| format!("TELEGRAM_BOT_TOKEN: {error}"))?;
        if telegram_bot_token.trim().is_empty() {
            return Err("TELEGRAM_BOT_TOKEN must not be empty".into());
        }
        Ok(Self {
            telegram_bot_token,
            database_url: env::var("DATABASE_URL")
                .map_err(|error| format!("DATABASE_URL: {error}"))?,
            bind_address: env::var("BIND_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
                .parse()?,
        })
    }
}
