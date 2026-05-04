use odnelazm::{HansardScraper, House, SittingListOptions};
use rmcp::{
    ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{ErrorData as McpError, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use chrono::NaiveDate;

#[derive(Debug, Clone)]
pub struct McpServer {
    scraper: HansardScraper,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl McpServer {
    pub fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {
            scraper: HansardScraper::new()?,
            tool_router: Self::tool_router(),
        })
    }

    #[tool(
        name = "list_sittings",
        description = "List parliamentary sittings with automatic source routing. If `end_date` is before 2013-03-28 the archive (info.mzalendo.com) is used. If `start_date` is on or after 2013-03-28 the current source (mzalendo.com) is used. If the range spans the cutoff — or one bound is absent while the other crosses it — both sources are queried in parallel and results are merged by date. With no dates, the current source is used with `page`/`all` pagination. Use `limit`/`offset` to slice the final result."
    )]
    pub async fn list_sittings(
        &self,
        Parameters(params): Parameters<ListSittingsParams>,
    ) -> Result<String, McpError> {
        if let Some(start) = params.start_date
            && let Some(end) = params.end_date
            && start > end
        {
            return Err(McpError::invalid_params(
                "start_date cannot be after end_date",
                None,
            ));
        }
        if params.offset.is_some_and(|o| o == 0) {
            return Err(McpError::invalid_params(
                "offset must be greater than 0",
                None,
            ));
        }
        if params.limit.is_some_and(|l| l == 0) {
            return Err(McpError::invalid_params(
                "limit must be greater than 0",
                None,
            ));
        }

        let listings = self
            .scraper
            .list_sittings(SittingListOptions {
                start_date: params.start_date,
                end_date: params.end_date,
                house: params.house,
                page: params.page.unwrap_or(1),
                all: params.all,
                limit: params.limit,
                offset: params.offset,
            })
            .await
            .inspect_err(|e| log::error!("Failed to fetch sittings: {e}"))
            .map_err(|e| {
                McpError::internal_error(format!("Failed to fetch sittings: {e}"), None)
            })?;

        serialize_list(listings)
    }

    #[tool(
        name = "get_sitting",
        description = "Fetch the full transcript of a parliamentary sitting, including sections, subsections, contributions, and procedural notes. The data source (archive or current) is detected automatically from the URL — archive URLs contain info.mzalendo.com, current URLs contain mzalendo.com/democracy-tools."
    )]
    pub async fn get_sitting(
        &self,
        Parameters(params): Parameters<GetSittingParams>,
    ) -> Result<String, McpError> {
        let sitting = self
            .scraper
            .get_sitting(&params.url_or_slug)
            .await
            .inspect_err(|e| log::error!("Failed to fetch sitting: {e}"))
            .map_err(|e| McpError::internal_error(format!("Failed to fetch sitting: {e}"), None))?;

        serde_json::to_string_pretty(&sitting).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize sitting: {e}"), None)
        })
    }

    #[tool(
        name = "list_members",
        description = "List members of parliament from the current source (mzalendo.com). Requires a house ('national_assembly' or 'senate') and parliament session (e.g. '13th-parliament'). Set `all` to true to fetch all pages at once."
    )]
    pub async fn list_members(
        &self,
        Parameters(params): Parameters<ListMembersParams>,
    ) -> Result<String, McpError> {
        let members = if params.all {
            self.scraper
                .list_all_members(params.house, &params.parliament)
                .await
                .inspect_err(|e| log::error!("Failed to fetch all members: {e}"))
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to fetch all members: {e}"), None)
                })?
        } else {
            let page = params.page.unwrap_or(1);
            self.scraper
                .list_members(params.house, &params.parliament, page)
                .await
                .inspect_err(|e| log::error!("Failed to fetch members page {page}: {e}"))
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to fetch members: {e}"), None)
                })?
        };

        serialize_list(members)
    }

    #[tool(
        name = "get_all_members",
        description = "Fetch all members of parliament from both houses (National Assembly and Senate) in parallel for a given parliament session. Use this when you need the full membership list or don't know which house a member belongs to. `parliament` defaults to '13th-parliament'."
    )]
    pub async fn get_all_members(
        &self,
        Parameters(params): Parameters<GetAllMembersParams>,
    ) -> Result<String, McpError> {
        let parliament = params.parliament.as_deref().unwrap_or("13th-parliament");

        let members = self
            .scraper
            .list_all_members_all_houses(parliament)
            .await
            .inspect_err(|e| log::error!("Failed to fetch all members (all houses): {e}"))
            .map_err(|e| {
                McpError::internal_error(format!("Failed to fetch all members: {e}"), None)
            })?;

        serialize_list(members)
    }

    #[tool(
        name = "get_member_profile",
        description = "Fetch a member of parliament's profile from the current source (mzalendo.com), including biography, positions, committees, voting patterns, parliamentary activity, and sponsored bills. Set `all_activity` or `all_bills` to true to exhaust all paginated data."
    )]
    pub async fn get_member_profile(
        &self,
        Parameters(params): Parameters<GetMemberProfileParams>,
    ) -> Result<String, McpError> {
        let profile = self
            .scraper
            .get_member_profile(&params.url_or_slug, params.all_activity, params.all_bills)
            .await
            .inspect_err(|e| log::error!("Failed to fetch member profile: {e}"))
            .map_err(|e| {
                McpError::internal_error(format!("Failed to fetch member profile: {e}"), None)
            })?;

        serde_json::to_string_pretty(&profile).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize profile: {e}"), None)
        })
    }
}

fn serialize_list<T: Serialize>(items: Vec<T>) -> Result<String, McpError> {
    let count = items.len();
    serde_json::to_string_pretty(&serde_json::json!({ "count": count, "data": items }))
        .map_err(|e| McpError::internal_error(format!("Failed to serialize list: {e}"), None))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListSittingsParams {
    /// Start of date range (YYYY-MM-DD).
    /// Setting this before 2013-03-28 while `end_date` is absent or on/after 2013-03-28
    /// triggers a cross-source merged query (archive + current fetched in parallel).
    pub start_date: Option<NaiveDate>,
    /// End of date range (YYYY-MM-DD).
    /// Before 2013-03-28 → archive only.
    /// On or after 2013-03-28 with `start_date` also before the cutoff → both sources merged.
    /// On or after 2013-03-28 with `start_date` absent or also on/after the cutoff → current only.
    pub end_date: Option<NaiveDate>,
    /// Filter by house: "senate" or "national_assembly".
    pub house: Option<House>,
    /// Page number for current-only queries (default: 1). Ignored for cross-source merged queries.
    pub page: Option<u32>,
    /// Fetch all pages at once for current-only queries. Ignored for cross-source merged queries.
    #[serde(default)]
    pub all: bool,
    /// Maximum results to return, applied after merging and sorting.
    pub limit: Option<usize>,
    /// Results to skip, applied after merging and sorting.
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetSittingParams {
    /// Full URL or slug of the sitting. Archive URLs contain info.mzalendo.com; current URLs contain mzalendo.com/democracy-tools.
    pub url_or_slug: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListMembersParams {
    /// House to list: "national_assembly" or "senate".
    pub house: House,
    /// Parliament session, e.g. "13th-parliament", "12th-parliament", "11th-parliament".
    pub parliament: String,
    /// Page number (default: 1). Ignored when `all` is true.
    pub page: Option<u32>,
    /// Fetch all pages at once.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetAllMembersParams {
    /// Parliament session. One of: "13th-parliament", "12th-parliament", "11th-parliament". Defaults to "13th-parliament".
    pub parliament: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMemberProfileParams {
    /// Full URL or slug of the member's profile page.
    pub url_or_slug: String,
    /// Fetch all pages of parliamentary activity (may be slow).
    #[serde(default)]
    pub all_activity: bool,
    /// Fetch all pages of sponsored bills (may be slow).
    #[serde(default)]
    pub all_bills: bool,
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
