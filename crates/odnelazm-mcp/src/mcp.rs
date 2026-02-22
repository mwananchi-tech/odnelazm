use odnelazm::archive::{scraper::WebScraper, utils::ListingFilter};
use rmcp::{
    ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{ErrorData as McpError, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

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

    #[tool(
        name = "list_sittings",
        description = "List available parliamentary sittings with optional filtering and pagination."
    )]
    pub async fn list_sittings(
        &self,
        Parameters(filters): Parameters<ListingFilter>,
    ) -> Result<String, McpError> {
        let filters = filters
            .validate()
            .inspect_err(|e| log::error!("Invalid params: {e:?}"))
            .map_err(|e| McpError::invalid_params(e, None))?;

        let listings = self
            .scraper
            .fetch_hansard_list()
            .await
            .inspect_err(|e| log::error!("Failed to fetch hansard list: {e:?}"))
            .map_err(|e| {
                McpError::internal_error(format!("Failed to fetch hansard list: {e:?}"), None)
            })?;

        let listings = filters.apply(listings);
        let json = serde_json::to_string_pretty(&listings)
            .inspect_err(|e| log::error!("Serialization error: {e:?}"))
            .map_err(|e| {
                McpError::internal_error(
                    format!("Failed to serialize hansard listings: {e:?}"),
                    None,
                )
            })?;

        Ok(json)
    }

    #[tool(
        name = "get_sitting",
        description = "Fetch the full transcript of a sitting including sections, contributions and procedural notes"
    )]
    pub async fn get_sitting(
        &self,
        Parameters(params): Parameters<GetSittingParams>,
    ) -> Result<String, McpError> {
        let sitting = self
            .scraper
            .fetch_hansard_detail(&params.url_or_slug, params.fetch_speakers)
            .await
            .inspect_err(|e| log::error!("Failed to fetch hansard detail: {e}"))
            .map_err(|e| McpError::internal_error(format!("Failed to fetch sitting: {e}"), None))?;

        Ok(sitting.to_string())
    }

    #[tool(
        name = "get_person",
        description = "Fetch speaker details from person profile pages"
    )]
    pub async fn get_person(
        &self,
        Parameters(params): Parameters<GetPersonParams>,
    ) -> Result<String, McpError> {
        let person = self
            .scraper
            .fetch_person_details(&params.url_or_slug)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to fetch sitting: {e}"), None))?;

        Ok(person.to_string())
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSittingParams {
    url_or_slug: String,
    fetch_speakers: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetPersonParams {
    url_or_slug: String,
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(include_str!("./instructions.md").to_string()),
            ..Default::default()
        }
    }
}
