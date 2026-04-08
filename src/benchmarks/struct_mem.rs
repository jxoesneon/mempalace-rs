use crate::benchmarks::{Benchmark, BenchmarkResult};
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;

/// StructMemEval: Organizational Prowess (Trees, States, Ledgers)
pub struct StructMemEval {
    pub use_hints: bool,
}

impl StructMemEval {
    pub fn new(use_hints: bool) -> Self {
        Self { use_hints }
    }

    async fn run_tree_traversal(&self, storage: &mut VectorStorage) -> Result<f64> {
        let wing = "struct_tree";
        let room = "hierarchy";
        let hint = if self.use_hints {
            "[ORGANIZATION: HIERARCHY] "
        } else {
            ""
        };

        storage.add_memory(
            &format!("{}Alice is the CEO.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}Bob reports to Alice.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}Charlie reports to Bob.", hint),
            wing,
            room,
            None,
            None,
        )?;

        let query = "Who is Alice's indirect report?";
        let results = storage.search_room(query, wing, room, 10, None)?;

        let found = results.iter().any(|r| r.text_content.contains("Charlie"));
        Ok(if found { 1.0 } else { 0.0 })
    }

    async fn run_state_tracking(&self, storage: &mut VectorStorage) -> Result<f64> {
        let wing = "struct_state";
        let room = "tracking";
        let hint = if self.use_hints {
            "[ORGANIZATION: STATE] "
        } else {
            ""
        };

        storage.add_memory(
            &format!("{}Server X is PENDING.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}Server X is now DEPLOYING.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}Server X is COMPLETED.", hint),
            wing,
            room,
            None,
            None,
        )?;

        let query = "What is the current state of Server X?";
        let results = storage.search_room(query, wing, room, 1, None)?;

        // Success if we find the LATEST state (Temporal awareness)
        let found = results.iter().any(|r| r.text_content.contains("COMPLETED"));
        Ok(if found { 1.0 } else { 0.0 })
    }

    async fn run_ledger_counting(&self, storage: &mut VectorStorage) -> Result<f64> {
        let wing = "struct_ledger";
        let room = "accounting";
        let hint = if self.use_hints {
            "[ORGANIZATION: LEDGER] "
        } else {
            ""
        };

        storage.add_memory(
            &format!("{}User A deposited $100.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}User A spent $30 on a sandwich.", hint),
            wing,
            room,
            None,
            None,
        )?;
        storage.add_memory(
            &format!("{}User A received a $10 refund.", hint),
            wing,
            room,
            None,
            None,
        )?;

        let query = "What is User A's final balance?";
        let results = storage.search_room(query, wing, room, 10, None)?;

        // Simple check: do we have all 3 entries to calculate 80?
        let count = results
            .iter()
            .filter(|r| r.text_content.contains("User A"))
            .count();
        Ok(if count == 3 { 1.0 } else { count as f64 / 3.0 })
    }
}

#[async_trait]
impl Benchmark for StructMemEval {
    fn name(&self) -> &str {
        "StructMemEval"
    }

    fn description(&self) -> &str {
        "Evaluates complex organizational memory structures (Trees, States, Ledgers)"
    }

    async fn run(&self, storage: &mut VectorStorage) -> Result<BenchmarkResult> {
        let start = Instant::now();

        let tree_score = self.run_tree_traversal(storage).await?;
        let state_score = self.run_state_tracking(storage).await?;
        let ledger_score = self.run_ledger_counting(storage).await?;

        let avg_score = (tree_score + state_score + ledger_score) / 3.0;
        let mut metadata = HashMap::new();
        metadata.insert("tree_traversal".to_string(), tree_score.to_string());
        metadata.insert("state_tracking".to_string(), state_score.to_string());
        metadata.insert("ledger_counting".to_string(), ledger_score.to_string());
        metadata.insert("use_hints".to_string(), self.use_hints.to_string());

        Ok(BenchmarkResult {
            name: self.name().to_string(),
            score: avg_score,
            metric_name: "Structural Integrity Score".to_string(),
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
    async fn test_struct_mem_run() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let db_path = temp_dir.path().join("test_struct.db");
        let index_path = temp_dir.path().join("test_struct.idx");

        let mut storage = VectorStorage::new(db_path, index_path)?;
        let benchmark = StructMemEval::new(true);

        let result = benchmark.run(&mut storage).await?;

        assert_eq!(result.name, "StructMemEval");
        assert!(result.score >= 0.0 && result.score <= 1.0);
        assert!(result.metadata.contains_key("tree_traversal"));
        assert!(result.metadata.contains_key("ledger_counting"));

        Ok(())
    }
}
