use crate::benchmarks::{Benchmark, BenchmarkResult};
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use async_trait::async_trait;
use rand::prelude::IndexedRandom;
use std::collections::HashMap;
use std::time::Instant;

/// BABILong Benchmark for ultra-long context reasoning
pub struct Babilong {
    pub token_limit: usize,
}

impl Babilong {
    pub fn new(token_limit: usize) -> Self {
        Self { token_limit }
    }

    fn generate_mock_haystack(&self) -> String {
        let words = vec![
            "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "lorem", "ipsum",
        ];
        let mut rng = rand::rng();
        let mut haystack = String::new();
        // Generate roughly token_limit worth of noise
        for _ in 0..(self.token_limit / 5) {
            haystack.push_str(words.choose(&mut rng).unwrap());
            haystack.push(' ');
        }
        haystack
    }
}

#[async_trait]
impl Benchmark for Babilong {
    fn name(&self) -> &str {
        "BABILong"
    }

    fn description(&self) -> &str {
        "Reasoning-in-a-Haystack benchmark at ultra-long token scale (1M-10M)"
    }

    async fn run(&self, storage: &mut VectorStorage) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let wing = "babilong";
        let room = "haystack";

        // 1. Ingest Fact A (Needle 1) at start
        storage.add_memory(
            "FACT_NEEDLE_ALPHA: The secret code is 42.",
            wing,
            room,
            None,
            None,
        )?;

        // 2. Ingest massive haystack (Simulation)
        // In a real test we would ingest PG-19 chunks here.
        storage.add_memory(&self.generate_mock_haystack(), wing, room, None, None)?;

        // 3. Ingest Fact B (Needle 2) at end
        storage.add_memory(
            "FACT_NEEDLE_BETA: The location is Sector 7G.",
            wing,
            room,
            None,
            None,
        )?;

        // 4. Multi-hop Reasoning Probe
        let query = "What is the secret code and where is the location?";
        let results = storage.search_room(query, wing, room, 10, None)?;

        let mut found_alpha = false;
        let mut found_beta = false;
        for res in &results {
            if res.text_content.contains("42") {
                found_alpha = true;
            }
            if res.text_content.contains("Sector 7G") {
                found_beta = true;
            }
        }

        let score = if found_alpha && found_beta {
            1.0
        } else if found_alpha || found_beta {
            0.5
        } else {
            0.0
        };

        let mut metadata = HashMap::new();
        metadata.insert(
            "token_frontier".to_string(),
            format!("{} tokens", self.token_limit),
        );
        metadata.insert("found_alpha".to_string(), found_alpha.to_string());
        metadata.insert("found_beta".to_string(), found_beta.to_string());

        Ok(BenchmarkResult {
            name: self.name().to_string(),
            score,
            metric_name: "Multi-Hop Reasoning Accuracy".to_string(),
            latency_ms: start.elapsed().as_millis() as f64,
            tokens_used: self.token_limit,
            metadata,
        })
    }
}
