use odnelazm::archive::{scraper::WebScraper as ArchiveScraper, utils::ListingFilter};
use odnelazm::current::scraper::WebScraper as CurrentScraper;
use odnelazm::types::House;
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
    archive_scraper: ArchiveScraper,
    current_scraper: CurrentScraper,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl McpServer {
    pub fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {
            archive_scraper: ArchiveScraper::new()?,
            current_scraper: CurrentScraper::new()?,
            tool_router: Self::tool_router(),
        })
    }

    #[tool(
        name = "archive_list_sittings",
        description = "List parliamentary sittings from the archive (info.mzalendo.com). Supports filtering by date range, house, limit, and offset. Use this for historical sitting data."
    )]
    pub async fn archive_list_sittings(
        &self,
        Parameters(filters): Parameters<ListingFilter>,
    ) -> Result<String, McpError> {
        let filters = filters
            .validate()
            .inspect_err(|e| log::error!("Invalid params: {e:?}"))
            .map_err(|e| McpError::invalid_params(e, None))?;

        let listings = self
            .archive_scraper
            .fetch_hansard_list()
            .await
            .inspect_err(|e| log::error!("Failed to fetch archive hansard list: {e:?}"))
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
        name = "archive_get_sitting",
        description = "Fetch the full transcript of an archived sitting (info.mzalendo.com), including sections, contributions, and procedural notes. Optionally fetch full speaker profiles inline."
    )]
    pub async fn archive_get_sitting(
        &self,
        Parameters(params): Parameters<ArchiveGetSittingParams>,
    ) -> Result<String, McpError> {
        let sitting = self
            .archive_scraper
            .fetch_hansard_sitting(&params.url_or_slug, params.fetch_speakers)
            .await
            .inspect_err(|e| log::error!("Failed to fetch archive sitting: {e}"))
            .map_err(|e| McpError::internal_error(format!("Failed to fetch sitting: {e}"), None))?;

        let json = serde_json::to_string_pretty(&sitting).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize sitting: {e}"), None)
        })?;

        Ok(json)
    }

    #[tool(
        name = "archive_get_person",
        description = "Fetch speaker/member details from an archived profile page (info.mzalendo.com), including party, constituency, and contact info."
    )]
    pub async fn archive_get_person(
        &self,
        Parameters(params): Parameters<ArchiveGetPersonParams>,
    ) -> Result<String, McpError> {
        let person = self
            .archive_scraper
            .fetch_person_details(&params.url_or_slug)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to fetch person: {e}"), None))?;

        let json = serde_json::to_string_pretty(&person).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize person: {e}"), None)
        })?;

        Ok(json)
    }

    #[tool(
        name = "current_list_sittings",
        description = "List recent parliamentary sittings from the current source (mzalendo.com). Supports house filtering and pagination. Set `all` to true to fetch all pages at once."
    )]
    pub async fn current_list_sittings(
        &self,
        Parameters(params): Parameters<CurrentListSittingsParams>,
    ) -> Result<String, McpError> {
        let listings = if params.all {
            self.current_scraper
                .fetch_all_sittings(params.house)
                .await
                .inspect_err(|e| log::error!("Failed to fetch all current sittings: {e}"))
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to fetch all sittings: {e}"), None)
                })?
        } else {
            let page = params.page.unwrap_or(1);
            self.current_scraper
                .fetch_hansard_list(page, params.house)
                .await
                .inspect_err(|e| log::error!("Failed to fetch current sittings page {page}: {e}"))
                .map_err(|e| {
                    McpError::internal_error(
                        format!("Failed to fetch sittings page {page}: {e}"),
                        None,
                    )
                })?
        };

        let json = serde_json::to_string_pretty(&listings).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize listings: {e}"), None)
        })?;

        Ok(json)
    }

    #[tool(
        name = "current_get_sitting",
        description = "Fetch the full transcript of a current sitting (mzalendo.com), including sections, contributions, and procedural notes."
    )]
    pub async fn current_get_sitting(
        &self,
        Parameters(params): Parameters<CurrentGetSittingParams>,
    ) -> Result<String, McpError> {
        let sitting = self
            .current_scraper
            .fetch_hansard_sitting(&params.url_or_slug)
            .await
            .inspect_err(|e| log::error!("Failed to fetch current sitting: {e}"))
            .map_err(|e| McpError::internal_error(format!("Failed to fetch sitting: {e}"), None))?;

        let json = serde_json::to_string_pretty(&sitting).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize sitting: {e}"), None)
        })?;

        Ok(json)
    }

    #[tool(
        name = "current_list_members",
        description = "List members of parliament from the current source (mzalendo.com). Requires a house and parliament session (e.g. '13th-parliament'). Set `all` to true to fetch all pages at once."
    )]
    pub async fn current_list_members(
        &self,
        Parameters(params): Parameters<CurrentListMembersParams>,
    ) -> Result<String, McpError> {
        let members = if params.all {
            self.current_scraper
                .fetch_all_members(params.house, &params.parliament)
                .await
                .inspect_err(|e| log::error!("Failed to fetch all members: {e}"))
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to fetch all members: {e}"), None)
                })?
        } else {
            let page = params.page.unwrap_or(1);
            self.current_scraper
                .fetch_members(params.house, &params.parliament, page)
                .await
                .inspect_err(|e| log::error!("Failed to fetch members page {page}: {e}"))
                .map_err(|e| {
                    McpError::internal_error(
                        format!("Failed to fetch members page {page}: {e}"),
                        None,
                    )
                })?
        };

        let json = serde_json::to_string_pretty(&members).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize members: {e}"), None)
        })?;

        Ok(json)
    }

    #[tool(
        name = "current_get_member_profile",
        description = "Fetch a member of parliament's profile from the current source (mzalendo.com), including biography, positions, committees, voting patterns, parliamentary activity, and sponsored bills. Set `all_activity` or `all_bills` to true to fetch all paginated data exhaustively."
    )]
    pub async fn current_get_member_profile(
        &self,
        Parameters(params): Parameters<CurrentGetMemberProfileParams>,
    ) -> Result<String, McpError> {
        let profile = self
            .current_scraper
            .fetch_member_profile(&params.url_or_slug, params.all_activity, params.all_bills)
            .await
            .inspect_err(|e| log::error!("Failed to fetch member profile: {e}"))
            .map_err(|e| {
                McpError::internal_error(format!("Failed to fetch member profile: {e}"), None)
            })?;

        let json = serde_json::to_string_pretty(&profile).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize profile: {e}"), None)
        })?;

        Ok(json)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ArchiveGetSittingParams {
    url_or_slug: String,
    fetch_speakers: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ArchiveGetPersonParams {
    url_or_slug: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CurrentListSittingsParams {
    page: Option<u32>,
    house: Option<House>,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CurrentGetSittingParams {
    url_or_slug: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CurrentListMembersParams {
    house: House,
    parliament: String,
    page: Option<u32>,
    #[serde(default)]
    all: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CurrentGetMemberProfileParams {
    url_or_slug: String,
    #[serde(default)]
    all_activity: bool,
    #[serde(default)]
    all_bills: bool,
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
