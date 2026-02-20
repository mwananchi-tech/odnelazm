mod parser;
pub mod scraper;
pub mod types;
pub mod utils;

pub use scraper::WebScraper;

pub(crate) const BASE_URL: &str = "https://info.mzalendo.com";
