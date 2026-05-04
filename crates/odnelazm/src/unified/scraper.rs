use chrono::NaiveDate;
use futures::future;

use crate::{
    archive::scraper::WebScraper as ArchiveScraper, current::scraper::WebScraper as CurrentScraper,
    types::House,
};

use super::types::{
    Bill, DataSource, HansardListing, HansardSitting, Member, MemberProfile, ParliamentaryActivity,
    SittingListOptions,
};

fn current_cutoff() -> NaiveDate {
    NaiveDate::from_ymd_opt(2013, 3, 28).expect("valid date")
}

impl DataSource {
    /// Detect the source from a URL or slug.
    /// Archive URLs contain `info.mzalendo.com` or the `/hansard/sitting/` path prefix.
    /// Everything else is treated as current.
    fn from_url(url_or_slug: &str) -> Self {
        if url_or_slug.contains("info.mzalendo.com") || url_or_slug.contains("/hansard/sitting/") {
            DataSource::Archive
        } else {
            DataSource::Current
        }
    }

    /// Resolve a URL or bare slug to a fully qualified URL for this source.
    fn normalize_url(&self, url_or_slug: &str) -> String {
        if url_or_slug.starts_with("http") {
            return url_or_slug.to_string();
        }
        match self {
            DataSource::Archive => format!("https://info.mzalendo.com{}", url_or_slug),
            DataSource::Current => {
                format!("https://mzalendo.com{}", url_or_slug.trim_end_matches('/'))
            }
        }
    }
}

enum ListingRoute {
    /// Only the archive covers this range.
    Archive,
    /// Only the current source covers this range.
    Current,
    /// The range spans the cutoff — fetch from both and merge.
    Both,
}

impl ListingRoute {
    /// Choose the route from a date range.
    ///
    /// | start_date  | end_date    | Route   |
    /// |-------------|-------------|---------|
    /// | any         | none        | Current (or Both if start < cutoff) |
    /// | —           | —           | Current |
    /// | —           | < cutoff    | Archive |
    /// | ≥ cutoff    | any         | Current |
    /// | < cutoff    | ≥ cutoff    | Both    |
    /// | < cutoff    | none        | Both    |
    fn from_dates(start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> Self {
        let cutoff = current_cutoff();
        match (start_date, end_date) {
            (None, None) => ListingRoute::Current,
            (_, Some(end)) if end < cutoff => ListingRoute::Archive,
            (Some(start), _) if start >= cutoff => ListingRoute::Current,
            _ => ListingRoute::Both,
        }
    }
}

impl SittingListOptions {
    /// Apply `offset` (skip) then `limit` (truncate) to `listings` in place.
    fn apply_slice(&self, listings: &mut Vec<HansardListing>) {
        if let Some(off) = self.offset {
            *listings = listings.drain(off..).collect();
        }
        if let Some(lim) = self.limit {
            listings.truncate(lim);
        }
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
    /// | Date range                              | Source          |
    /// |-----------------------------------------|-----------------|
    /// | No dates                                | Current (paged) |
    /// | `end_date` before 2013-03-28            | Archive         |
    /// | `start_date` on or after 2013-03-28     | Current         |
    /// | Spans the cutoff (or one bound missing) | Both, merged    |
    ///
    /// When both sources are queried they are fetched in parallel. Results are
    /// merged and sorted by date descending before `limit`/`offset` are applied.
    /// `page`/`all` apply only when the route is Current-only.
    pub async fn list_sittings(
        &self,
        opts: SittingListOptions,
    ) -> Result<Vec<HansardListing>, ScraperError> {
        match ListingRoute::from_dates(opts.start_date, opts.end_date) {
            ListingRoute::Archive => {
                let mut listings = self
                    .fetch_archive_listings(opts.start_date, opts.end_date, opts.house)
                    .await?;
                opts.apply_slice(&mut listings);
                Ok(listings)
            }

            ListingRoute::Current => {
                let has_date_filter = opts.start_date.is_some() || opts.end_date.is_some();
                let raw = if opts.all || has_date_filter {
                    self.current.fetch_all_sittings(opts.house).await?
                } else {
                    self.current
                        .fetch_hansard_list(opts.page.max(1), opts.house)
                        .await?
                };
                let mut listings: Vec<HansardListing> =
                    raw.into_iter().map(HansardListing::from).collect();
                if let Some(start) = opts.start_date {
                    listings.retain(|l| l.date >= start);
                }
                if let Some(end) = opts.end_date {
                    listings.retain(|l| l.date <= end);
                }
                opts.apply_slice(&mut listings);
                Ok(listings)
            }

            ListingRoute::Both => {
                log::info!(
                    "Date range spans the 2013-03-28 cutoff — querying archive and current source in parallel"
                );
                let cutoff = current_cutoff();

                let (archive_result, current_result) = future::join(
                    self.fetch_archive_listings(
                        opts.start_date,
                        Some(cutoff - chrono::Days::new(1)),
                        opts.house,
                    ),
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
                        let mut current_listings: Vec<HansardListing> =
                            items.into_iter().map(HansardListing::from).collect();
                        if let Some(end) = opts.end_date {
                            current_listings.retain(|l| l.date <= end);
                        }
                        listings.extend(current_listings);
                    }
                    Err(e) => log::warn!("Current fetch failed during cross-source query: {e}"),
                }

                listings.sort_by_key(|l| std::cmp::Reverse(l.date));
                opts.apply_slice(&mut listings);
                Ok(listings)
            }
        }
    }

    /// Fetch the full transcript of a sitting by URL or slug.
    /// The data source is detected automatically from the URL shape.
    pub async fn get_sitting(&self, url_or_slug: &str) -> Result<HansardSitting, ScraperError> {
        let source = DataSource::from_url(url_or_slug);
        let url = source.normalize_url(url_or_slug);
        match source {
            DataSource::Archive => {
                let sitting = self.archive.fetch_hansard_sitting(&url, false).await?;
                Ok(HansardSitting::from_archive(sitting, url))
            }
            DataSource::Current => {
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
        Ok(self
            .current
            .fetch_all_members_all_houses(parliament)
            .await?)
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
        Ok(self
            .current
            .fetch_member_activity(url_or_slug, page)
            .await?)
    }

    pub async fn get_member_bills(
        &self,
        url_or_slug: &str,
        page: u32,
    ) -> Result<Vec<Bill>, ScraperError> {
        Ok(self.current.fetch_member_bills(url_or_slug, page).await?)
    }

    /// Fetch archive listings and apply date-range and house filters client-side.
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
