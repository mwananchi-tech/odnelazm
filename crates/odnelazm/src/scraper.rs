use crate::parser::{ParseError, parse_hansard_detail, parse_hansard_list, parse_person_details};
use crate::types::{HansardDetail, HansardListing, PersonDetails};

use reqwest::Client;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
    #[error("Page not found: {0}")]
    NotFound(String),
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
            base_url: crate::BASE_URL.to_string(),
        })
    }

    pub async fn fetch_hansard_list(&self) -> Result<Vec<HansardListing>, ScraperError> {
        let url = format!("{}/hansard/", self.base_url);
        let html = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let listings = parse_hansard_list(&html)?;
        Ok(listings)
    }

    pub async fn fetch_hansard_detail(&self, url: &str) -> Result<HansardDetail, ScraperError> {
        let html = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        if html.contains("Page Not Found") || html.contains("404") {
            return Err(ScraperError::NotFound(url.into()));
        }

        let detail = parse_hansard_detail(&html, url)?;
        Ok(detail)
    }

    pub async fn fetch_person_details(&self, url: &str) -> Result<PersonDetails, ScraperError> {
        let full_url = if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", self.base_url, url)
        };

        let html = self
            .client
            .get(&full_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        if html.trim().is_empty() {
            return Err(ScraperError::ParseError(ParseError::MissingField(format!(
                "Empty response for {}",
                url
            ))));
        }

        let details = parse_person_details(&html, url)?;
        Ok(details)
    }
}
