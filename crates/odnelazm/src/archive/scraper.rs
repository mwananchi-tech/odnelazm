use super::parser::{ParseError, parse_hansard_detail, parse_hansard_list, parse_person_details};
use super::types::{HansardDetail, HansardListing, PersonDetails};

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
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

    pub async fn fetch_hansard_list(&self) -> Result<Vec<HansardListing>, ScraperError> {
        log::info!("Fetching hansard listings...");

        let url = format!("{}/hansard/", self.base_url);
        let html = self
            .client
            .get(&url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?
            .text()
            .await
            .inspect_err(|e| log::error!("Decode error: {e:?}"))?;

        let listings = parse_hansard_list(&html)?;
        Ok(listings)
    }

    pub async fn fetch_hansard_detail(
        &self,
        url_or_slug: &str,
        nest_speaker_fetch: bool,
    ) -> Result<HansardDetail, ScraperError> {
        let url = if url_or_slug.starts_with("http") {
            url_or_slug.to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug)
        };

        log::info!("Fetching hansard details...");

        let html = self
            .client
            .get(&url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?
            .text()
            .await
            .inspect_err(|e| log::error!("Decode error: {e:?}"))?;

        let mut sitting = parse_hansard_detail(&html, &url)?;

        if nest_speaker_fetch {
            let speaker_urls: HashSet<String> = sitting
                .sections
                .iter()
                .flat_map(|s| &s.contributions)
                .filter_map(|c| c.speaker_url.as_ref())
                .cloned()
                .collect();

            if !speaker_urls.is_empty() {
                log::info!("Fetching {} speaker profiles...", speaker_urls.len());

                let mut futures: FuturesUnordered<_> = speaker_urls
                    .iter()
                    .map(|url| async move { (url, self.fetch_person_details(url).await) })
                    .collect();

                let mut speaker_map = HashMap::new();
                while let Some((url, result)) = futures.next().await {
                    match result {
                        Ok(details) => {
                            speaker_map.insert(url.clone(), details);
                        }
                        Err(e) => log::warn!("Failed to fetch speaker {}: {}", url, e),
                    }
                }

                for contrib in sitting
                    .sections
                    .iter_mut()
                    .flat_map(|s| &mut s.contributions)
                {
                    if let Some(url) = &contrib.speaker_url {
                        contrib.speaker_details = speaker_map.get(url).cloned();
                    }
                }

                log::info!(
                    "Successfully fetched {} speaker profiles",
                    speaker_map.len()
                );
            }
        } else {
            log::info!("Nested speaker profile fetch skipped");
        }

        Ok(sitting)
    }

    pub async fn fetch_person_details(
        &self,
        url_or_slug: &str,
    ) -> Result<PersonDetails, ScraperError> {
        let url = if url_or_slug.starts_with("http") {
            url_or_slug.to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug)
        };

        let html = self
            .client
            .get(&url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?
            .text()
            .await
            .inspect_err(|e| log::error!("Decode error: {e:?}"))?;

        if html.trim().is_empty() {
            return Err(ScraperError::ParseError(ParseError::MissingField(format!(
                "Empty response for {}",
                url
            ))));
        }

        let details = parse_person_details(&html, &url)?;
        Ok(details)
    }
}
