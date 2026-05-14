use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::summarize::{Summarizer, SummaryContext, build_prompt};
use crate::{IngestError, Result};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct ChatResponse {
    output: Vec<OutputItem>,
}

#[derive(Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    r#type: String,
    content: String,
}

#[derive(Debug, Clone)]
pub struct LmStudioSummarizer {
    client: Client,
    base_url: String,
    model: String,
    temperature: Option<f32>,
}

impl LmStudioSummarizer {
    pub fn new(base_url: &str, model: &str, temperature: f32) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            temperature: Some(temperature),
        }
    }

    pub async fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/api/v1/chat", self.base_url);
        let body = ChatRequest {
            model: self.model.clone(),
            input: prompt.to_string(),
            system_prompt: None,
            temperature: self.temperature,
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| IngestError::Embed(e.to_string()))?
            .error_for_status()
            .map_err(|e| IngestError::Embed(e.to_string()))?
            .json::<ChatResponse>()
            .await
            .map_err(|e| IngestError::Embed(e.to_string()))?;

        // Qwen3 returns a reasoning block followed by the actual message.
        // Always use the last item with type "message".
        resp.output
            .into_iter()
            .rfind(|o| o.r#type == "message")
            .map(|o| o.content.trim().to_string())
            .ok_or_else(|| IngestError::Embed("LLM returned no message output".into()))
    }
}

#[async_trait]
impl Summarizer for LmStudioSummarizer {
    async fn summarize(&self, ctx: &SummaryContext, contributions_text: &str) -> Result<String> {
        let prompt = build_prompt(ctx, contributions_text);
        self.complete(&prompt).await
    }
}
