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
    async fn summarize(&self, ctx: &SummaryContext, contributions_text: &str) -> Result<String>;
}

/// Build a ready-to-send prompt from the context and contribution text.
/// Exposed so implementors can use it directly or customise it.
pub fn build_prompt(ctx: &SummaryContext, contributions_text: &str) -> String {
    let stage_line = ctx
        .stage
        .as_deref()
        .map(|s| format!("Stage: {s}\n"))
        .unwrap_or_default();

    format!(
        "You are analysing parliamentary contributions from the Parliament of Kenya.\n\
         \n\
         Member: {member}\n\
         {item_type_label}: {title}\n\
         {stage_line}\
         House: {house}\n\
         Date: {date}\n\
         \n\
         The member's contributions during this debate:\n\
         ---\n\
         {text}\n\
         ---\n\
         \n\
         In 2–3 sentences, summarise this member's position, key arguments, and any \
         notable statements. Be factual and concise. Do not invent details.",
        member = ctx.member_name,
        item_type_label = if ctx.item_type == "bill" {
            "Bill"
        } else {
            "Topic"
        },
        title = ctx.title,
        stage_line = stage_line,
        house = ctx.house,
        date = ctx.date,
        text = contributions_text,
    )
}
