use std::io;

use odnelazm_mcp::McpServer;
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8055";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .target(env_logger::Target::Stderr)
        .write_style(env_logger::WriteStyle::Never)
        .init();

    let ct = tokio_util::sync::CancellationToken::new();

    let service = StreamableHttpService::new(
        || {
            McpServer::new()
                .map_err(|e| io::Error::other(format!("Failed to init mcp server: {e:?}")))
        },
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            cancellation_token: ct.child_token(),
            ..Default::default()
        },
    );

    let address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.into());
    let router = axum::Router::new().nest_service("/sse", service);
    let tcp_listener = tokio::net::TcpListener::bind(&address).await?;

    log::info!("Starting mcp server on address: {}", address);

    let _ = axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.unwrap();
            ct.cancel();
        })
        .await;

    Ok(())
}
