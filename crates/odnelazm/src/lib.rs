pub(crate) mod archive;
pub(crate) mod current;
pub mod types;
pub mod unified;

pub use types::House;
pub use unified::scraper::{HansardScraper, ScraperError};
pub use unified::types::{
    Bill, Contribution, DataSource, HansardListing, HansardSection, HansardSubsection,
    HansardSitting, Member, MemberProfile, ParliamentaryActivity, SittingListOptions, VoteRecord,
};
