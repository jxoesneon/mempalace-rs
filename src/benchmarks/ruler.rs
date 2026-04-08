use crate::benchmarks::{Benchmark, BenchmarkResult};
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;

/// RULER (Realistic and Universal LLM Evaluation with Long-Contexts)
pub struct Ruler {
    pub k: usize,
}

impl Ruler {
    pub fn new(k: usize) -> Self {
        Self { k }
    }

    async fn run_variable_tracking(&self, storage: &mut VectorStorage) -> Result<(f64, f64)> {
        let wing = "ruler_var";
        let room = "tracking";

        // 1. Ingest 10 distinct variables
        for i in 0..10 {
            storage.add_memory(
                &format!(
                    "The value of variable_alpha_{} is data_point_{}.",
                    i,
                    i * 100
                ),
                wing,
                room,
                None,
                None,
            )?;
        }

        // 2. Query each variable
        let mut hits = 0;
        let mut total_ndcg = 0.0;
        let idcg = 1.0; // Since only one item is relevant for each query, ideal rank is 0

        for i in 0..10 {
            let query = format!("What is the value of variable_alpha_{}?", i);
            let results = storage.search_room(&query, wing, room, self.k, None)?;

            for (rank, res) in results.iter().enumerate() {
                if res
                    .text_content
                    .contains(&format!("data_point_{}", i * 100))
                {
                    hits += 1;
                    let dcg = 1.0 / (rank as f64 + 2.0).log2();
                    total_ndcg += dcg / idcg;
                    break;
                }
            }
        }

        let recall = hits as f64 / 10.0;
        let ndcg = total_ndcg / 10.0;

        Ok((recall, ndcg))
    }

    async fn run_aggregation(&self, storage: &mut VectorStorage) -> Result<(f64, f64)> {
        let wing = "ruler_agg";
        let room = "counting";

        // 1. Ingest 5 instances of 'Target Entity'
        for i in 0..5 {
            storage.add_memory(
                &format!("Instance {} of the target entity is active here.", i),
                wing,
                room,
                None,
                None,
            )?;
        }
        // Add 20 noise memories
        for i in 0..20 {
            storage.add_memory(
                &format!("Noise memory #{} contains irrelevant info.", i),
                wing,
                room,
                None,
                None,
            )?;
        }

        // 2. Query for all instances
        let query = "Show me all instances of the target entity.";
        let results = storage.search_room(query, wing, room, self.k, None)?;

        let mut found_count = 0;
        for res in &results {
            if res.text_content.contains("target entity") {
                found_count += 1;
            }
        }

        let recall = found_count as f64 / 5.0;
        // Simple nDCG for aggregation is less standard, using binary relevance
        let ndcg = recall;

        Ok((recall, ndcg))
    }
}

#[async_trait]
impl Benchmark for Ruler {
    fn name(&self) -> &str {
        "RULER"
    }

    fn description(&self) -> &str {
        "Realistic and Universal LLM Evaluation with Long-Contexts (Multi-Needle & Aggregation)"
    }

    async fn run(&self, storage: &mut VectorStorage) -> Result<BenchmarkResult> {
        let start = Instant::now();

        let (var_recall, var_ndcg) = self.run_variable_tracking(storage).await?;
        let (agg_recall, agg_ndcg) = self.run_aggregation(storage).await?;

        let avg_score = (var_ndcg + agg_ndcg) / 2.0;
        let mut metadata = HashMap::new();
        metadata.insert(
            "variable_tracking_recall".to_string(),
            var_recall.to_string(),
        );
        metadata.insert("variable_tracking_ndcg".to_string(), var_ndcg.to_string());
        metadata.insert("aggregation_recall".to_string(), agg_recall.to_string());

        Ok(BenchmarkResult {
            name: self.name().to_string(),
            score: avg_score,
            metric_name: "RULER-Score (nDCG)".to_string(),
            latency_ms: start.elapsed().as_millis() as f64,
            tokens_used: 0,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_storage::VectorStorage;

    #[tokio::test]
    async fn test_ruler_run() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let db_path = temp_dir.path().join("test_ruler.db");
        let index_path = temp_dir.path().join("test_ruler.idx");

        let mut storage = VectorStorage::new(db_path, index_path)?;
        let benchmark = Ruler::new(10);

        let result = benchmark.run(&mut storage).await?;

        assert_eq!(result.name, "RULER");
        assert!(result.score >= 0.0 && result.score <= 1.0);
        assert!(result.metadata.contains_key("variable_tracking_recall"));
        assert!(result.metadata.contains_key("aggregation_recall"));

        Ok(())
    }
}
