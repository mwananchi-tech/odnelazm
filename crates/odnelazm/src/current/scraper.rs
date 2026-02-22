use super::parser::{
    ParseError, parse_activity_page_info, parse_bills, parse_bills_page_info, parse_hansard_list,
    parse_hansard_sitting, parse_member_list, parse_member_profile, parse_page_info,
    parse_parliamentary_activity,
};
use super::types::{
    Bill, HansardListing, HansardSitting, House, Member, MemberProfile, ParliamentaryActivity,
};

use futures::stream::FuturesUnordered;
use futures::{StreamExt, future};
use reqwest::Client;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
    #[error("Page {requested} is out of range (last page is {last})")]
    PageOutOfRange { requested: u32, last: u32 },
}

#[derive(Debug, Clone)]
pub struct WebScraper {
    client: Client,
    base_url: String,
}

impl WebScraper {
    pub fn new() -> Result<Self, ScraperError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!(
                "{}/{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;

        Ok(Self {
            client,
            base_url: super::BASE_URL.to_string(),
        })
    }

    pub async fn fetch_hansard_list(
        &self,
        page: u32,
        house: Option<House>,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        let url = format!("{}/democracy-tools/hansard/?page={}", self.base_url, page);
        log::info!("Fetching hansard list page {}...", page);
        let html = self.get_html(&url).await?;
        self.check_page(page, &html)?;
        Ok(parse_hansard_list(&html, house)?)
    }

    pub async fn fetch_hansard_sitting(
        &self,
        url_or_slug: &str,
    ) -> Result<HansardSitting, ScraperError> {
        let url = if url_or_slug.starts_with("http") {
            url_or_slug.to_string()
        } else {
            format!(
                "{}/democracy-tools/hansard/{}/",
                self.base_url,
                url_or_slug.trim_matches('/')
            )
        };
        log::info!("Fetching hansard sitting: {}", url);
        let html = self.get_html(&url).await?;
        Ok(parse_hansard_sitting(&html, &url)?)
    }

    pub async fn fetch_members(
        &self,
        house: House,
        parliament: &str,
        page: u32,
    ) -> Result<Vec<Member>, ScraperError> {
        let url = format!(
            "{}/mps-performance/{}/{}/?q=&page={}",
            self.base_url,
            house.slug(),
            parliament,
            page
        );
        log::info!(
            "Fetching {} members ({}, page {})...",
            house.slug(),
            parliament,
            page
        );
        let html = self.get_html(&url).await?;
        self.check_page(page, &html)?;
        Ok(parse_member_list(&html, house)?)
    }

    pub async fn fetch_member_profile(
        &self,
        url_or_slug: &str,
        fetch_all_activity: bool,
        fetch_all_bills: bool,
    ) -> Result<MemberProfile, ScraperError> {
        let url = if url_or_slug.starts_with("http") {
            url_or_slug.to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug)
        };
        log::info!("Fetching member profile: {}", url);
        let html = self.get_html(&url).await?;
        let mut profile = parse_member_profile(&html, &url)?;

        let (extra_activity, extra_bills) = future::join(
            async {
                if fetch_all_activity && profile.activity_pages > 1 {
                    log::info!(
                        "Fetching {} remaining activity page(s)...",
                        profile.activity_pages - 1
                    );
                    let mut futs: FuturesUnordered<_> = (2..=profile.activity_pages)
                        .map(|page| self.fetch_member_activity(&url, page))
                        .collect();
                    let mut all = Vec::new();
                    while let Some(result) = futs.next().await {
                        match result {
                            Ok(items) => all.extend(items),
                            Err(e) => log::warn!("Failed to fetch activity page: {}", e),
                        }
                    }
                    all
                } else {
                    Vec::new()
                }
            },
            async {
                if fetch_all_bills && profile.bills_pages > 1 {
                    log::info!(
                        "Fetching {} remaining bills page(s)...",
                        profile.bills_pages - 1
                    );
                    let mut futs: FuturesUnordered<_> = (2..=profile.bills_pages)
                        .map(|page| self.fetch_member_bills(&url, page))
                        .collect();
                    let mut all = Vec::new();
                    while let Some(result) = futs.next().await {
                        match result {
                            Ok(items) => all.extend(items),
                            Err(e) => log::warn!("Failed to fetch bills page: {}", e),
                        }
                    }
                    all
                } else {
                    Vec::new()
                }
            },
        )
        .await;

        profile.activity.extend(extra_activity);
        profile.bills.extend(extra_bills);

        Ok(profile)
    }

    pub async fn fetch_member_activity(
        &self,
        url_or_slug: &str,
        contributions_page: u32,
    ) -> Result<Vec<ParliamentaryActivity>, ScraperError> {
        let base = if url_or_slug.starts_with("http") {
            url_or_slug.trim_end_matches('/').to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug.trim_end_matches('/'))
        };
        let url = format!("{}/?contributions_page={}", base, contributions_page);
        log::info!(
            "Fetching member activity page {}: {}",
            contributions_page,
            url
        );
        let html = self.get_html(&url).await?;
        if let Some((current, last)) = parse_activity_page_info(&html)
            && current != contributions_page
        {
            return Err(ScraperError::PageOutOfRange {
                requested: contributions_page,
                last,
            });
        }
        Ok(parse_parliamentary_activity(&html))
    }

    pub async fn fetch_member_bills(
        &self,
        url_or_slug: &str,
        bills_page: u32,
    ) -> Result<Vec<Bill>, ScraperError> {
        let base = if url_or_slug.starts_with("http") {
            url_or_slug.trim_end_matches('/').to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug.trim_end_matches('/'))
        };
        let url = format!("{}/?bills_page={}", base, bills_page);
        log::info!("Fetching member bills page {}: {}", bills_page, url);
        let html = self.get_html(&url).await?;
        if let Some((current, last)) = parse_bills_page_info(&html)
            && current != bills_page
        {
            return Err(ScraperError::PageOutOfRange {
                requested: bills_page,
                last,
            });
        }
        Ok(parse_bills(&html))
    }

    fn check_page(&self, requested: u32, html: &str) -> Result<(), ScraperError> {
        if let Some((current, last)) = parse_page_info(html)
            && current != requested
        {
            return Err(ScraperError::PageOutOfRange { requested, last });
        }
        Ok(())
    }

    async fn get_html(&self, url: &str) -> Result<String, ScraperError> {
        Ok(self
            .client
            .get(url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?
            .text()
            .await
            .inspect_err(|e| log::error!("Decode error: {e:?}"))?)
    }
}
