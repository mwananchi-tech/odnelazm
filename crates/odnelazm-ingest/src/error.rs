use odnelazm::ScraperError;

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("Store error: {0}")]
    Store(String),
    #[error("Scraper error: {0}")]
    Scraper(#[from] ScraperError),
    #[error("Embed error: {0}")]
    Embed(String),
    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl From<sqlx::Error> for IngestError {
    fn from(e: sqlx::Error) -> Self {
        IngestError::Store(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, IngestError>;
