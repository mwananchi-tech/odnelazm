use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use reqwest::Client;

use super::MetricsSink;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MetricKind {
    Counter,
    Gauge,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MetricKey {
    name: String,
    labels: Vec<(String, String)>,
    kind: MetricKind,
}

/// Pushes metrics to a Prometheus pushgateway in text exposition format.
///
/// Metrics accumulate in memory and are sent together on `flush()`.
/// If the pushgateway is unreachable, a warning is logged and ingestion
/// continues — metrics are best-effort.
pub struct PrometheusPushSink {
    client: Client,
    url: String,
    state: Mutex<HashMap<MetricKey, f64>>,
}

impl PrometheusPushSink {
    /// `pushgateway_url` — base URL of the pushgateway, e.g. `http://localhost:9091`
    /// `job` — job label used to group metrics in the pushgateway
    pub fn new(pushgateway_url: &str, job: &str) -> Self {
        Self {
            client: Client::new(),
            url: format!(
                "{}/metrics/job/{}",
                pushgateway_url.trim_end_matches('/'),
                job
            ),
            state: Mutex::new(HashMap::new()),
        }
    }

    fn format_body(state: &HashMap<MetricKey, f64>) -> String {
        // Group entries by (name, kind) to emit one # TYPE header per metric.
        let mut groups: HashMap<(&str, &MetricKind), Vec<(&MetricKey, f64)>> = HashMap::new();
        for (key, &value) in state {
            groups
                .entry((&key.name, &key.kind))
                .or_default()
                .push((key, value));
        }

        let mut body = String::new();
        for ((name, kind), entries) in &groups {
            let type_str = match kind {
                MetricKind::Counter => "counter",
                MetricKind::Gauge => "gauge",
            };
            body.push_str(&format!("# TYPE {name} {type_str}\n"));
            for (key, value) in entries {
                if key.labels.is_empty() {
                    body.push_str(&format!("{name} {value}\n"));
                } else {
                    let labels = key
                        .labels
                        .iter()
                        .map(|(k, v)| format!("{k}=\"{v}\""))
                        .collect::<Vec<_>>()
                        .join(",");
                    body.push_str(&format!("{name}{{{labels}}} {value}\n"));
                }
            }
        }
        body
    }
}

#[async_trait]
impl MetricsSink for PrometheusPushSink {
    fn counter(&self, name: &str, value: u64, labels: &[(&str, &str)]) {
        let key = MetricKey {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            kind: MetricKind::Counter,
        };
        *self.state.lock().unwrap().entry(key).or_default() += value as f64;
    }

    fn gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let key = MetricKey {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            kind: MetricKind::Gauge,
        };
        self.state.lock().unwrap().insert(key, value);
    }

    async fn flush(&self) {
        let body = Self::format_body(&self.state.lock().unwrap());
        if body.is_empty() {
            return;
        }
        if let Err(e) = self
            .client
            .post(&self.url)
            .header("Content-Type", "text/plain; version=0.0.4")
            .body(body)
            .send()
            .await
        {
            log::warn!("Failed to push metrics to Prometheus pushgateway: {e}");
        }
    }
}
