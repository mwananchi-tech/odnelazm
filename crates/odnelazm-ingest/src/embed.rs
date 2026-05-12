use async_trait::async_trait;

use crate::Result;

/// Trait for generating embeddings from text.
///
/// Implement with your preferred provider — OpenAI, a local model, Anthropic
/// (once they expose an embedding endpoint), etc. The pipeline calls this
/// once per sitting using the text produced by [`sitting_text`].
#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Dimensionality of the vectors produced, e.g. 1536 for
    /// text-embedding-3-small. Stored for documentation; the pipeline does
    /// not enforce it.
    fn dimensions(&self) -> usize;
}

/// Build the text that gets embedded for a sitting.
///
/// Uses `summary` when available (current sittings always have one), otherwise
/// falls back to a concatenation of section and subsection titles. The sitting
/// metadata (house, date, session type) is always prepended for grounding.
pub fn sitting_text(sitting: &odnelazm::HansardSitting) -> String {
    let header = format!(
        "{} — {} — {}",
        sitting.house, sitting.date, sitting.session_type
    );

    let body = if let Some(summary) = &sitting.summary {
        summary.clone()
    } else {
        sitting
            .sections
            .iter()
            .flat_map(|s| {
                let mut titles = vec![s.section_type.clone()];
                titles.extend(s.subsections.iter().map(|sub| sub.title.clone()));
                titles
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!("{header}\n\n{body}")
}
