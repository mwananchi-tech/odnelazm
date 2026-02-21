use odnelazm_mcp::McpServer;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .target(env_logger::Target::Stderr)
        .write_style(env_logger::WriteStyle::Never)
        .init();

    log::info!("Starting odnelazm MCP server");

    let service = McpServer::new()?.serve(stdio()).await.inspect_err(|e| {
        log::error!("Serve error: {e:?}");
    })?;

    service.waiting().await?;

    Ok(())
}
