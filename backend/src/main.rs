use cocina_dashboard::{Config, app, initialize};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    let address = config.listen.clone();
    let state = initialize(config).await?;
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "Dashboard ready");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
