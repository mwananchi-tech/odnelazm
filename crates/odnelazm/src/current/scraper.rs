use super::parser::{
    ParseError, parse_activity_page_info, parse_bills, parse_bills_page_info, parse_hansard_list,
    parse_hansard_page_identity, parse_hansard_pdf_url, parse_member_list, parse_member_profile,
    parse_page_info, parse_parliamentary_activity,
};
use super::pdf::{self, PdfError};
use super::types::{
    Bill, HansardListing, HansardSitting, House, Member, MemberProfile, ParliamentaryActivity,
};

use futures::stream::FuturesUnordered;
use futures::{StreamExt, future};
use reqwest::{Client, Url, redirect::Policy};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
    #[error("PDF error: {0}")]
    PdfError(#[from] PdfError),
    #[error("Document exceeds the 50 MiB download limit")]
    DocumentTooLarge,
    #[error("Unsupported Hansard PDF host: {0}")]
    UnsupportedPdfHost(String),
    #[error("Page {requested} is out of range (last page is {last})")]
    PageOutOfRange { requested: u32, last: u32 },
}

#[derive(Debug, Clone)]
pub struct WebScraper {
    client: Client,
    base_url: String,
    /// Preserves listing metadata because the sitting API receives only a URL.
    listings: Arc<Mutex<HashMap<String, HansardListing>>>,
}

impl WebScraper {
    pub fn new() -> Result<Self, ScraperError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("too many redirects")
                } else if is_trusted_pdf_host(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirected to an unsupported host")
                }
            }))
            .user_agent(format!(
                "{}/{}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;

        Ok(Self {
            client,
            base_url: super::BASE_URL.to_string(),
            listings: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn fetch_hansard_list(
        &self,
        page: u32,
        house: Option<House>,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        let url = format!("{}/democracy-tools/hansard/?page={}", self.base_url, page);
        log::debug!("Fetching hansard list page {}...", page);
        let html = self.get_html(&url).await?;
        self.check_page(page, &html)?;
        let listings = parse_hansard_list(&html, house)?;
        self.remember_listings(&listings);
        Ok(listings)
    }

    pub async fn fetch_all_sittings(
        &self,
        house: Option<House>,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        let first_url = format!("{}/democracy-tools/hansard/?page=1", self.base_url);
        let first_html = self.get_html(&first_url).await?;
        let total_pages = parse_page_info(&first_html)?
            .map(|(_, total)| total)
            .unwrap_or(1);
        let mut listings = parse_hansard_list(&first_html, house)?;
        self.remember_listings(&listings);

        if total_pages > 1 {
            log::info!(
                "Fetching {} remaining hansard list page(s)...",
                total_pages - 1
            );
            let mut futs: FuturesUnordered<_> = (2..=total_pages)
                .map(|page| self.fetch_hansard_list(page, house))
                .collect();
            while let Some(result) = futs.next().await {
                extend_page(&mut listings, result)?;
            }
        }

        listings.sort_by_key(|l| std::cmp::Reverse(l.date));
        Ok(listings)
    }

    pub async fn fetch_hansard_sitting(
        &self,
        url_or_slug: &str,
    ) -> Result<HansardSitting, ScraperError> {
        let url = if url_or_slug.starts_with("http") {
            url_or_slug.to_string()
        } else {
            format!("{}{}", self.base_url, url_or_slug.trim_end_matches('/'))
        };
        log::info!("Fetching hansard sitting: {}", url);
        let remembered_identity = self.remembered_identity(&url);
        let (pdf_url, page_identity) = if is_pdf_url(&url) {
            (url.clone(), remembered_identity)
        } else {
            let html = self.get_html(&url).await?;
            let identity = match parse_hansard_page_identity(&html) {
                Ok(identity) => identity,
                Err(error) => remembered_identity.ok_or(error)?,
            };
            (parse_hansard_pdf_url(&html, &url)?, Some(identity))
        };
        if !is_trusted_pdf_url(&pdf_url) {
            return Err(ScraperError::UnsupportedPdfHost(pdf_url));
        }
        log::info!("Fetching hansard PDF: {}", pdf_url);
        let pdf_bytes = self.get_bytes(&pdf_url).await?;
        let text = pdf::extract_text(pdf_bytes).await?;
        let mut sitting = pdf::parse_sitting(&text, &pdf_url)?;
        if let Some(identity) = page_identity {
            if identity.house != sitting.house || identity.date != sitting.date {
                return Err(PdfError::Validation(format!(
                    "Mzalendo listing identifies {:?} {}, but PDF identifies {:?} {}",
                    identity.house, identity.date, sitting.house, sitting.date
                ))
                .into());
            }
            if !identity.session_type.is_empty() {
                sitting.session_type = identity.session_type;
            }
            sitting.summary = identity.summary;
            sitting.sentiment = identity.sentiment;
        }
        Ok(sitting)
    }

    pub async fn fetch_members(
        &self,
        house: House,
        parliament: &str,
        page: u32,
    ) -> Result<Vec<Member>, ScraperError> {
        let url = format!(
            "{}/mps-performance/{}/{}/?page={}",
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

    pub async fn fetch_all_members(
        &self,
        house: House,
        parliament: &str,
    ) -> Result<Vec<Member>, ScraperError> {
        let first_url = format!(
            "{}/mps-performance/{}/{}/?page=1",
            self.base_url,
            house.slug(),
            parliament
        );
        let first_html = self.get_html(&first_url).await?;
        let total_pages = parse_page_info(&first_html)?
            .map(|(_, total)| total)
            .unwrap_or(1);
        let mut members = parse_member_list(&first_html, house)?;

        if total_pages > 1 {
            log::info!(
                "Fetching {} remaining {} member page(s)...",
                total_pages - 1,
                house.slug()
            );
            let mut futs: FuturesUnordered<_> = (2..=total_pages)
                .map(|page| self.fetch_members(house, parliament, page))
                .collect();
            while let Some(result) = futs.next().await {
                extend_page(&mut members, result)?;
            }
        }

        deduplicate_members(&mut members);
        Ok(members)
    }

    pub async fn fetch_all_members_all_houses(
        &self,
        parliament: &str,
    ) -> Result<Vec<Member>, ScraperError> {
        let (na_result, senate_result) = future::join(
            self.fetch_all_members(House::NationalAssembly, parliament),
            self.fetch_all_members(House::Senate, parliament),
        )
        .await;

        let mut members = na_result?;
        members.extend(senate_result?);

        Ok(members)
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
                    let mut all = Ok(Vec::new());
                    while let Some(result) = futs.next().await {
                        match (&mut all, result) {
                            (Ok(items), Ok(page_items)) => items.extend(page_items),
                            (slot @ Ok(_), Err(error)) => *slot = Err(error),
                            (Err(_), _) => {}
                        }
                    }
                    all
                } else {
                    Ok(Vec::new())
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
                    let mut all = Ok(Vec::new());
                    while let Some(result) = futs.next().await {
                        match (&mut all, result) {
                            (Ok(items), Ok(page_items)) => items.extend(page_items),
                            (slot @ Ok(_), Err(error)) => *slot = Err(error),
                            (Err(_), _) => {}
                        }
                    }
                    all
                } else {
                    Ok(Vec::new())
                }
            },
        )
        .await;

        profile.activity.extend(extra_activity?);
        profile.bills.extend(extra_bills?);

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
        log::debug!(
            "Fetching member activity page {}: {}",
            contributions_page,
            url
        );
        let html = self.get_html(&url).await?;
        if let Some((current, last)) = parse_activity_page_info(&html)?
            && current != contributions_page
        {
            return Err(ScraperError::PageOutOfRange {
                requested: contributions_page,
                last,
            });
        }
        Ok(parse_parliamentary_activity(&html)?)
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
        log::debug!("Fetching member bills page {}: {}", bills_page, url);
        let html = self.get_html(&url).await?;
        if let Some((current, last)) = parse_bills_page_info(&html)?
            && current != bills_page
        {
            return Err(ScraperError::PageOutOfRange {
                requested: bills_page,
                last,
            });
        }
        Ok(parse_bills(&html)?)
    }

    fn check_page(&self, requested: u32, html: &str) -> Result<(), ScraperError> {
        if let Some((current, last)) = parse_page_info(html)?
            && current != requested
        {
            return Err(ScraperError::PageOutOfRange { requested, last });
        }
        Ok(())
    }

    async fn get_html(&self, url: &str) -> Result<String, ScraperError> {
        let html = self
            .client
            .get(url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?
            .text()
            .await
            .inspect_err(|e| log::error!("Decode error: {e:?}"))?;

        Ok(html)
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, ScraperError> {
        const MAX_PDF_BYTES: usize = 50 * 1024 * 1024;

        let response = self
            .client
            .get(url)
            .send()
            .await
            .inspect_err(|e| log::error!("HTTP error: {e:?}"))?
            .error_for_status()?;
        if !is_trusted_pdf_host(response.url()) {
            return Err(ScraperError::UnsupportedPdfHost(response.url().to_string()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PDF_BYTES as u64)
        {
            return Err(ScraperError::DocumentTooLarge);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if bytes.len().saturating_add(chunk.len()) > MAX_PDF_BYTES {
                return Err(ScraperError::DocumentTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    fn remember_listings(&self, listings: &[HansardListing]) {
        let Ok(mut remembered) = self.listings.lock() else {
            return;
        };
        for listing in listings {
            let url = if listing.url.starts_with("http") {
                listing.url.clone()
            } else {
                format!("{}{}", self.base_url, listing.url)
            };
            remembered.insert(url.trim_end_matches('/').to_owned(), listing.clone());
        }
    }

    fn remembered_identity(&self, url: &str) -> Option<super::parser::HansardPageIdentity> {
        let remembered = self.listings.lock().ok()?;
        let listing = remembered.get(url.trim_end_matches('/'))?;
        Some(super::parser::HansardPageIdentity {
            house: listing.house,
            date: listing.date,
            session_type: listing.session_type.clone(),
            summary: None,
            sentiment: None,
        })
    }
}

fn is_pdf_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.to_ascii_lowercase().ends_with(".pdf") || path.ends_with("/source/")
}

fn is_trusted_pdf_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| is_trusted_pdf_host(&url))
}

fn is_trusted_pdf_host(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        let official_host = ["mzalendo.com", "parliament.go.ke", "senate.go.ke"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
        let mzalendo_storage = host == "mzdocs.fra1.digitaloceanspaces.com"
            || (host == "fra1.digitaloceanspaces.com" && url.path().starts_with("/mzdocs/"));
        official_host || mzalendo_storage
    })
}

fn deduplicate_members(members: &mut Vec<Member>) {
    let mut seen = HashSet::new();
    members.retain(|member| seen.insert(member.url.clone()));
}

fn extend_page<T>(
    items: &mut Vec<T>,
    page: Result<Vec<T>, ScraperError>,
) -> Result<(), ScraperError> {
    items.extend(page?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_listing_identity_for_direct_pdf_fetches() {
        let scraper = WebScraper::new().unwrap();
        let url = "https://www.parliament.go.ke/hansard.pdf";
        scraper.remember_listings(&[HansardListing {
            house: House::Senate,
            date: chrono::NaiveDate::from_ymd_opt(2026, 8, 20).unwrap(),
            session_type: "Evening Sitting".to_owned(),
            url: url.to_owned(),
            title: "Thursday, 20th August, 2026 - Evening Sitting".to_owned(),
            kind: super::super::types::HansardListingKind::Transcript,
        }]);

        let identity = scraper.remembered_identity(url).unwrap();
        assert_eq!(identity.house, House::Senate);
        assert_eq!(identity.date.to_string(), "2026-08-20");
        assert_eq!(identity.session_type, "Evening Sitting");
    }

    #[test]
    fn normalizes_listing_cache_trailing_slashes() {
        let scraper = WebScraper::new().unwrap();
        scraper.remember_listings(&[HansardListing {
            house: House::Senate,
            date: chrono::NaiveDate::from_ymd_opt(2026, 8, 5).unwrap(),
            session_type: "Afternoon Sitting".to_owned(),
            url: "/democracy-tools/hansard/document/3157/".to_owned(),
            title: "Wednesday, 5th August, 2026 - Afternoon Sitting".to_owned(),
            kind: super::super::types::HansardListingKind::Transcript,
        }]);

        assert!(
            scraper
                .remembered_identity("https://mzalendo.com/democracy-tools/hansard/document/3157")
                .is_some()
        );
    }

    #[test]
    fn restricts_pdf_downloads_to_official_hosts() {
        assert!(is_trusted_pdf_url(
            "https://www.parliament.go.ke/sites/default/files/hansard.pdf"
        ));
        assert!(is_trusted_pdf_url(
            "https://mzalendo.com/democracy-tools/hansard/document/3157/source/"
        ));
        assert!(is_trusted_pdf_url(
            "https://fra1.digitaloceanspaces.com/mzdocs/prod/media/hansard.pdf"
        ));
        assert!(!is_trusted_pdf_url("https://127.0.0.1/hansard.pdf"));
        assert!(!is_trusted_pdf_url(
            "http://www.parliament.go.ke/hansard.pdf"
        ));
        assert!(!is_trusted_pdf_url(
            "https://fra1.digitaloceanspaces.com/another-bucket/hansard.pdf"
        ));
        assert!(!is_trusted_pdf_url(
            "https://parliament.go.ke.attacker.example/hansard.pdf"
        ));
    }

    #[test]
    fn deduplicates_members_by_url_and_preserves_first_record() {
        let url = "/mps-performance/national-assembly/13th-parliament/example/";
        let mut members = vec![
            Member {
                name: "Example Leader".to_string(),
                url: url.to_string(),
                house: House::NationalAssembly,
                role: Some("Majority Leader".to_string()),
                constituency: Some("Kikuyu".to_string()),
            },
            Member {
                name: "Example Leader".to_string(),
                url: url.to_string(),
                house: House::NationalAssembly,
                role: None,
                constituency: Some("Kikuyu".to_string()),
            },
            Member {
                name: "Another Member".to_string(),
                url: "/mps-performance/national-assembly/13th-parliament/another/".to_string(),
                house: House::NationalAssembly,
                role: None,
                constituency: Some("Ijara".to_string()),
            },
        ];

        deduplicate_members(&mut members);

        assert_eq!(members.len(), 2);
        assert_eq!(members[0].role.as_deref(), Some("Majority Leader"));
        assert_eq!(members[1].name, "Another Member");
    }

    #[test]
    fn failed_pagination_page_is_propagated() {
        let mut items = vec![1];
        let error = ScraperError::PageOutOfRange {
            requested: 2,
            last: 1,
        };

        assert!(extend_page(&mut items, Err(error)).is_err());
        assert_eq!(items, vec![1]);
    }
}
