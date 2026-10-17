use std::sync::Arc;

use rmcp::{ServiceExt, transport::stdio};
use yc_cashflow_mcp::{
    server::YcServer,
    source::{Config, Store},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout carries JSON-RPC; logs must go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let store = Arc::new(Store::from_config(Config::from_env())?);
    let service = YcServer::new(store).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
