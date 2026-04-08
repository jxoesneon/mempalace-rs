use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod babilong;
pub mod beam;
pub mod judge;
pub mod metrics;
pub mod ruler;
pub mod struct_mem;

pub use ruler::Ruler;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub score: f64,
    pub metric_name: String,
    pub latency_ms: f64,
    pub tokens_used: usize,
    pub metadata: HashMap<String, String>,
}

#[async_trait]
pub trait Benchmark: Send + Sync {
    /// Unique name of the benchmark (e.g., "RULER-1M")
    fn name(&self) -> &str;

    /// Execute the benchmark across the provided VectorStorage
    async fn run(
        &self,
        storage: &mut crate::vector_storage::VectorStorage,
    ) -> Result<BenchmarkResult>;

    /// Return a description of the benchmark's purpose
    fn description(&self) -> &str;
}
