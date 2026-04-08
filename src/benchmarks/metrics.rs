use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MetricsStore {
    pub results: Vec<crate::benchmarks::BenchmarkResult>,
    pub environment: HashMap<String, String>,
}

impl MetricsStore {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            environment: HashMap::new(),
        }
    }

    pub fn add_result(&mut self, result: crate::benchmarks::BenchmarkResult) {
        self.results.push(result);
    }

    pub fn save_to_file(&self, path: &str) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
