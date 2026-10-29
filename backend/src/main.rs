use backend::{
    api::{AppState, router},
    auth::TelegramAuth,
    config::Config,
    db::connect_and_migrate,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let pool = connect_and_migrate(&config.database_url).await?;
    let state = AppState {
        pool: pool.clone(),
        auth: TelegramAuth::new(&config.telegram_bot_token),
    };
    let listener = tokio::net::TcpListener::bind(config.bind_address).await?;
    println!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("failed to listen for shutdown signal: {error}");
            }
        })
        .await?;
    pool.close().await;
    Ok(())
}
