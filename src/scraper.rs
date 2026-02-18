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
}

pub struct WebScraper {
    client: Client,
    base_url: String,
}

impl WebScraper {
    pub fn new() -> Result<Self, ScraperError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("odnelazm/0.1.0")
            .build()?;

        Ok(Self {
            client,
            base_url: "https://info.mzalendo.com".to_string(),
        })
    }

    pub async fn fetch_hansard_list(&self) -> Result<Vec<HansardListing>, ScraperError> {
        let url = format!("{}/hansard/", self.base_url);
        let html = self.client.get(&url).send().await?.text().await?;
        let listings = parse_hansard_list(&html)?;
        Ok(listings)
    }

    pub async fn fetch_hansard_detail(&self, url: &str) -> Result<String, ScraperError> {
        let html = self.client.get(url).send().await?.text().await?;
        Ok(html)
    }

    pub async fn fetch_hansard_detail_parsed(
        &self,
        url: &str,
    ) -> Result<HansardDetail, ScraperError> {
        let html = self.fetch_hansard_detail(url).await?;
        let detail = parse_hansard_detail(&html, url)?;
        Ok(detail)
    }

    pub async fn fetch_person_details(&self, url: &str) -> Result<PersonDetails, ScraperError> {
        let full_url = if url.starts_with("http") {
            url.to_string()
        } else {
            format!("{}{}", self.base_url, url)
        };
        let response = self.client.get(&full_url).send().await?;

        if !response.status().is_success() {
            return Err(ScraperError::ParseError(ParseError::MissingField(format!(
                "HTTP {}: {}",
                response.status(),
                url
            ))));
        }

        let html = response.text().await?;

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

impl Default for WebScraper {
    fn default() -> Self {
        Self::new().expect("Failed to create WebScraper")
    }
}
