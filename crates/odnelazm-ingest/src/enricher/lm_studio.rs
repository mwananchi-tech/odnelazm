use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::metrics::MetricsSink;
use crate::summarize::Summarizer;
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
    #[serde(default)]
    stats: Option<ResponseStats>,
}

#[derive(Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    r#type: String,
    content: String,
}

#[derive(Deserialize)]
struct ResponseStats {
    input_tokens: Option<u64>,
    total_output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    tokens_per_second: Option<f64>,
    time_to_first_token_seconds: Option<f64>,
}

pub struct LmStudioSummarizer {
    client: Client,
    base_url: String,
    model: String,
    temperature: Option<f32>,
    metrics: Option<Arc<dyn MetricsSink>>,
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
            metrics: None,
        }
    }

    pub fn with_metrics(mut self, sink: Arc<dyn MetricsSink>) -> Self {
        self.metrics = Some(sink);
        self
    }

    fn emit_stats(&self, stats: &ResponseStats) {
        let Some(metrics) = &self.metrics else { return };
        let labels: &[(&str, &str)] = &[("model", &self.model)];
        if let Some(v) = stats.input_tokens {
            metrics.counter("llm_input_tokens", v, labels);
        }
        if let Some(v) = stats.total_output_tokens {
            metrics.counter("llm_output_tokens", v, labels);
        }
        if let Some(v) = stats.reasoning_output_tokens {
            metrics.counter("llm_reasoning_tokens", v, labels);
        }
        if let Some(v) = stats.tokens_per_second {
            metrics.gauge("llm_tokens_per_second", v, labels);
        }
        if let Some(v) = stats.time_to_first_token_seconds {
            metrics.gauge("llm_time_to_first_token_seconds", v, labels);
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

        if let Some(stats) = &resp.stats {
            self.emit_stats(stats);
        }

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
    async fn summarize(&self, prompt: &str) -> Result<String> {
        self.complete(prompt).await
    }
}
