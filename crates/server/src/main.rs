use server::{Config, run};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "server=info,tower_http=info".to_string()),
        )
        .init();

    run(Config::from_env()).await
}
