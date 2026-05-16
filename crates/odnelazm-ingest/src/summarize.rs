use async_trait::async_trait;
use chrono::NaiveDate;

use crate::Result;

/// Context passed to the summarizer alongside the raw contribution text.
#[derive(Debug, Clone)]
pub struct SummaryContext {
    pub member_name: String,
    /// Bill name or topic title.
    pub title: String,
    /// "bill" | "topic"
    pub item_type: String,
    /// Legislative stage for bills (e.g. "Second Reading"), None for topics.
    pub stage: Option<String>,
    pub date: NaiveDate,
    pub house: String,
}

/// Generate a brief summary of a member's contributions to a bill or topic.
///
/// Implement this trait with your preferred LLM provider. The pipeline calls
/// [`Summarizer::summarize`] once per (member, bill/topic) pair that has
/// `contributions_text` but no `summary` yet.
#[async_trait]
pub trait Summarizer: Send + Sync {
    /// Send a prompt and return the model's response.
    /// The caller is responsible for building the full prompt with all relevant context.
    async fn summarize(&self, prompt: &str) -> Result<String>;
}
