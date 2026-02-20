use odnelazm::scraper::WebScraper;
use rmcp::{
    ServerHandler,
    handler::server::tool::ToolRouter,
    model::{
        CallToolResult, ErrorData as McpError, Implementation, ProtocolVersion, ServerCapabilities,
        ServerInfo,
    },
    tool, tool_handler, tool_router,
};

#[derive(Debug, Clone)]
pub struct McpServer {
    scraper: WebScraper,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl McpServer {
    pub fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {
            scraper: WebScraper::new()?,
            tool_router: Self::tool_router(),
        })
    }

    #[tool(description = "List all parliamentary sessions")]
    pub async fn list_sittings(&self) -> Result<CallToolResult, McpError> {
        todo!()
    }

    #[tool(description = "Get the full hansard of a given sitting")]
    pub async fn get_sitting(&self) -> Result<CallToolResult, McpError> {
        todo!()
    }

    #[tool(description = "Get additional details of a speaker/member")]
    pub async fn get_person(&self) -> Result<CallToolResult, McpError> {
        todo!()
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(include_str!("./instructions.md").to_string()),
        }
    }
}
