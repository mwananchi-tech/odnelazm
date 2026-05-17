pub mod prometheus;

use async_trait::async_trait;

/// A sink that receives pipeline metrics.
///
/// `counter` and `gauge` update in-memory state synchronously.
/// `flush` sends accumulated metrics to the configured backend — call it
/// at the end of each batch or pipeline run. On a `NoopSink` all three
/// methods are zero-cost.
///
/// Not configuring a sink never breaks ingestion: the pipeline uses
/// `NoopSink` by default.
#[async_trait]
pub trait MetricsSink: Send + Sync {
    fn counter(&self, name: &str, value: u64, labels: &[(&str, &str)]);
    fn gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]);
    async fn flush(&self);
}

pub struct NoopSink;

#[async_trait]
impl MetricsSink for NoopSink {
    fn counter(&self, _: &str, _: u64, _: &[(&str, &str)]) {}
    fn gauge(&self, _: &str, _: f64, _: &[(&str, &str)]) {}
    async fn flush(&self) {}
}
