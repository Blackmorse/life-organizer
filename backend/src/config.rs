use std::{env, net::SocketAddr};

pub struct Config {
    pub database_url: String,
    pub bind_address: SocketAddr,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            bind_address: env::var("BIND_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
                .parse()?,
        })
    }
}
