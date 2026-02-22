mod parser;
pub mod scraper;
pub mod types;

pub use scraper::{ScraperError, WebScraper};

pub(crate) const BASE_URL: &str = "https://mzalendo.com";
