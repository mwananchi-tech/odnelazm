pub mod embed;
pub mod enricher;
pub mod error;
pub mod extract;
pub mod pipeline;
pub mod postgres;
pub mod store;
pub mod summarize;

pub use embed::Embedder;
pub use error::{IngestError, Result};
pub use pipeline::{IngestPipeline, IngestStats};
pub use postgres::PostgresStore;
pub use store::DataStore;
pub use summarize::{Summarizer, SummaryContext, build_prompt};
