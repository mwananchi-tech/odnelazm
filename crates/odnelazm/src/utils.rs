use crate::types::{HansardListing, House};

use chrono::NaiveDate;

#[derive(Debug, Default)]
pub struct ListingFilter {
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub house: Option<House>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl ListingFilter {
    pub fn apply(self, mut listings: Vec<HansardListing>) -> Vec<HansardListing> {
        if let Some(start) = self.start_date {
            listings.retain(|l| l.date >= start);
        }
        if let Some(end) = self.end_date {
            listings.retain(|l| l.date <= end);
        }
        if let Some(house) = self.house {
            listings.retain(|l| l.house == house);
        }
        if let Some(off) = self.offset {
            listings = listings.into_iter().skip(off).collect();
        }
        if let Some(lim) = self.limit {
            listings.truncate(lim);
        }
        listings
    }

    pub fn validate(self) -> Result<Self, String> {
        if let Some(start) = self.start_date
            && let Some(end) = self.end_date
            && start > end
        {
            return Err(format!(
                "Start date ({start}) cannot be after end date ({end})"
            ));
        }
        if self.offset.is_some_and(|o| o == 0) {
            return Err("Offset must be greater than 0".to_string());
        }
        if self.limit.is_some_and(|l| l == 0) {
            return Err("Limit must be greater than 0".to_string());
        }
        Ok(self)
    }
}

#[derive(Debug)]
pub struct ListingStats {
    pub senate: usize,
    pub national_assembly: usize,
    pub total: usize,
}

impl ListingStats {
    pub fn from_hansard_listings(listings: &[HansardListing]) -> ListingStats {
        ListingStats {
            senate: listings.iter().filter(|l| l.house == House::Senate).count(),
            national_assembly: listings
                .iter()
                .filter(|l| l.house == House::NationalAssembly)
                .count(),
            total: listings.len(),
        }
    }
}

impl std::fmt::Display for ListingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\nStatistics:")?;
        writeln!(f, "  Senate sittings:            {}", self.senate)?;
        writeln!(
            f,
            "  National Assembly sittings: {}",
            self.national_assembly
        )?;
        writeln!(f, "  Total:                      {}", self.total)
    }
}
