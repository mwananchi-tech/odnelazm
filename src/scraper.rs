use crate::parser::{parse_hansard_list, ParseError};
use crate::types::HansardListing;
use reqwest::blocking::Client;
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

    pub fn fetch_hansard_list(&self) -> Result<Vec<HansardListing>, ScraperError> {
        let url = format!("{}/hansard/", self.base_url);
        let html = self.client.get(&url).send()?.text()?;
        let listings = parse_hansard_list(&html)?;
        Ok(listings)
    }

    pub fn fetch_hansard_detail(&self, url: &str) -> Result<String, ScraperError> {
        let html = self.client.get(url).send()?.text()?;
        Ok(html)
    }
}

impl Default for WebScraper {
    fn default() -> Self {
        Self::new().expect("Failed to create WebScraper")
    }
}

