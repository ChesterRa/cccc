use anyhow::Result;
use cccc_core::HomeLayout;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let host = std::env::var("CCCC_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("CCCC_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8848);
    cccc_web::serve(HomeLayout::resolve()?, &host, port).await?;
    Ok(())
}
