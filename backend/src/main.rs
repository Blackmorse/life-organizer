use backend::{
    api::{AppState, router},
    auth::TelegramAuth,
    config::Config,
    db::connect_and_migrate,
    speech::openai::OpenAiSpeechRecognizer,
    telegram::TelegramSender,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let speech = std::sync::Arc::new(OpenAiSpeechRecognizer::new(config.openai_key)?);
    let pool = connect_and_migrate(&config.database_url).await?;
    let bot = std::sync::Arc::new(TelegramSender::new(&config.telegram_bot_token)?);
    let state = AppState {
        notifications: bot.clone(),
        pool: pool.clone(),
        auth: TelegramAuth::new(&config.telegram_bot_token),
    };
    let listener = tokio::net::TcpListener::bind(config.bind_address).await?;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(backend::morning::run(pool.clone(), bot, speech, stopped));
    println!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("failed to listen for shutdown signal: {error}");
            }
        })
        .await?;
    let _ = stop.send(true);
    worker.await?;
    pool.close().await;
    Ok(())
}
