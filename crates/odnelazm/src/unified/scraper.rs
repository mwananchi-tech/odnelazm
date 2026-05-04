use chrono::NaiveDate;
use futures::future;

use crate::{
    archive::scraper::WebScraper as ArchiveScraper,
    current::scraper::WebScraper as CurrentScraper,
    types::House,
};

use super::types::{
    Bill, DataSource, HansardListing, HansardSitting, Member, MemberProfile, ParliamentaryActivity,
    SittingListOptions,
};

/// Sittings from this date onward are served by the current source (mzalendo.com).
/// Sittings before this date are served by the archive (info.mzalendo.com).
fn current_cutoff() -> NaiveDate {
    NaiveDate::from_ymd_opt(2013, 3, 28).expect("valid date")
}

enum ListingRoute {
    /// Only the archive covers this range.
    Archive,
    /// Only the current source covers this range.
    Current,
    /// The range spans the cutoff — fetch from both and merge.
    Both,
}

/// Determine which source(s) to query.
///
/// Rules:
/// - No dates at all → Current (paginated recent listing).
/// - `end_date` before the cutoff → Archive.
/// - `start_date` at or after the cutoff → Current.
/// - Anything else (range spans the cutoff, or one bound is missing while the
///   other crosses it) → Both.
fn route_listing(start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> ListingRoute {
    let cutoff = current_cutoff();
    match (start_date, end_date) {
        (None, None) => ListingRoute::Current,
        (_, Some(end)) if end < cutoff => ListingRoute::Archive,
        (Some(start), _) if start >= cutoff => ListingRoute::Current,
        _ => ListingRoute::Both,
    }
}

/// Route a sitting lookup by inspecting the URL or slug.
/// Archive URLs contain info.mzalendo.com or the /hansard/sitting/ path.
/// Everything else is treated as current.
fn detect_sitting_source(url_or_slug: &str) -> DataSource {
    if url_or_slug.contains("info.mzalendo.com") || url_or_slug.contains("/hansard/sitting/") {
        DataSource::Archive
    } else {
        DataSource::Current
    }
}

fn to_archive_url(url_or_slug: &str) -> String {
    if url_or_slug.starts_with("http") {
        url_or_slug.to_string()
    } else {
        format!("https://info.mzalendo.com{}", url_or_slug)
    }
}

fn to_current_url(url_or_slug: &str) -> String {
    if url_or_slug.starts_with("http") {
        url_or_slug.to_string()
    } else {
        format!(
            "https://mzalendo.com{}",
            url_or_slug.trim_end_matches('/')
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error(transparent)]
    Archive(#[from] crate::archive::scraper::ScraperError),
    #[error(transparent)]
    Current(#[from] crate::current::scraper::ScraperError),
}

#[derive(Debug, Clone)]
pub struct HansardScraper {
    archive: ArchiveScraper,
    current: CurrentScraper,
}

impl HansardScraper {
    pub fn new() -> Result<Self, ScraperError> {
        Ok(Self {
            archive: ArchiveScraper::new()?,
            current: CurrentScraper::new()?,
        })
    }

    /// List parliamentary sittings with automatic source routing.
    ///
    /// | Date range                              | Source        |
    /// |-----------------------------------------|---------------|
    /// | No dates                                | Current (paged) |
    /// | `end_date` before 2013-03-28            | Archive       |
    /// | `start_date` on or after 2013-03-28     | Current       |
    /// | Spans the cutoff (or one bound missing) | Both, merged  |
    ///
    /// When both sources are queried they are fetched in parallel. Results are
    /// merged and sorted by date descending before `limit`/`offset` are applied.
    /// `page`/`all` apply only when the route is Current-only.
    pub async fn list_sittings(
        &self,
        opts: SittingListOptions,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        match route_listing(opts.start_date, opts.end_date) {
            ListingRoute::Archive => {
                let mut listings = self.fetch_archive_listings(opts.start_date, opts.end_date, opts.house).await?;
                apply_slice(&mut listings, opts.offset, opts.limit);
                Ok(listings)
            }

            ListingRoute::Current => {
                let raw = if opts.all {
                    self.current.fetch_all_sittings(opts.house).await?
                } else {
                    self.current
                        .fetch_hansard_list(opts.page.max(1), opts.house)
                        .await?
                };
                Ok(raw.into_iter().map(HansardListing::from).collect())
            }

            ListingRoute::Both => {
                log::info!(
                    "Date range spans the 2013-03-28 cutoff — querying archive and current source in parallel"
                );
                let cutoff = current_cutoff();

                let (archive_result, current_result) = future::join(
                    self.fetch_archive_listings(opts.start_date, Some(cutoff - chrono::Days::new(1)), opts.house),
                    self.current.fetch_all_sittings(opts.house),
                )
                .await;

                let mut listings: Vec<HansardListing> = Vec::new();

                match archive_result {
                    Ok(items) => listings.extend(items),
                    Err(e) => log::warn!("Archive fetch failed during cross-source query: {e}"),
                }

                match current_result {
                    Ok(items) => {
                        let mut current_listings: Vec<HansardListing> = items
                            .into_iter()
                            .map(HansardListing::from)
                            .collect();
                        // Filter current results to the requested end_date
                        if let Some(end) = opts.end_date {
                            current_listings.retain(|l| l.date <= end);
                        }
                        listings.extend(current_listings);
                    }
                    Err(e) => log::warn!("Current fetch failed during cross-source query: {e}"),
                }

                listings.sort_by_key(|l| std::cmp::Reverse(l.date));
                apply_slice(&mut listings, opts.offset, opts.limit);
                Ok(listings)
            }
        }
    }

    /// Fetch the full transcript of a sitting by URL or slug.
    /// The data source is detected automatically from the URL shape.
    pub async fn get_sitting(&self, url_or_slug: &str) -> Result<HansardSitting, ScraperError> {
        match detect_sitting_source(url_or_slug) {
            DataSource::Archive => {
                let url = to_archive_url(url_or_slug);
                let sitting = self.archive.fetch_hansard_sitting(&url, false).await?;
                Ok(HansardSitting::from_archive(sitting, url))
            }
            DataSource::Current => {
                let url = to_current_url(url_or_slug);
                let sitting = self.current.fetch_hansard_sitting(&url).await?;
                Ok(HansardSitting::from_current(sitting, url))
            }
        }
    }

    pub async fn list_members(
        &self,
        house: House,
        parliament: &str,
        page: u32,
    ) -> Result<Vec<Member>, ScraperError> {
        Ok(self.current.fetch_members(house, parliament, page).await?)
    }

    pub async fn list_all_members(
        &self,
        house: House,
        parliament: &str,
    ) -> Result<Vec<Member>, ScraperError> {
        Ok(self.current.fetch_all_members(house, parliament).await?)
    }

    pub async fn list_all_members_all_houses(
        &self,
        parliament: &str,
    ) -> Result<Vec<Member>, ScraperError> {
        Ok(self.current.fetch_all_members_all_houses(parliament).await?)
    }

    pub async fn get_member_profile(
        &self,
        url_or_slug: &str,
        all_activity: bool,
        all_bills: bool,
    ) -> Result<MemberProfile, ScraperError> {
        Ok(self
            .current
            .fetch_member_profile(url_or_slug, all_activity, all_bills)
            .await?)
    }

    pub async fn get_member_activity(
        &self,
        url_or_slug: &str,
        page: u32,
    ) -> Result<Vec<ParliamentaryActivity>, ScraperError> {
        Ok(self.current.fetch_member_activity(url_or_slug, page).await?)
    }

    pub async fn get_member_bills(
        &self,
        url_or_slug: &str,
        page: u32,
    ) -> Result<Vec<Bill>, ScraperError> {
        Ok(self.current.fetch_member_bills(url_or_slug, page).await?)
    }

    /// Fetch archive listings and filter client-side by date range and house.
    async fn fetch_archive_listings(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        house: Option<House>,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        let mut listings: Vec<HansardListing> = self
            .archive
            .fetch_hansard_list()
            .await?
            .into_iter()
            .map(HansardListing::from)
            .collect();

        if let Some(start) = start_date {
            listings.retain(|l| l.date >= start);
        }
        if let Some(end) = end_date {
            listings.retain(|l| l.date <= end);
        }
        if let Some(h) = house {
            listings.retain(|l| l.house == h);
        }

        Ok(listings)
    }
}

fn apply_slice(listings: &mut Vec<HansardListing>, offset: Option<usize>, limit: Option<usize>) {
    if let Some(off) = offset {
        *listings = listings.drain(off..).collect();
    }
    if let Some(lim) = limit {
        listings.truncate(lim);
    }
}
