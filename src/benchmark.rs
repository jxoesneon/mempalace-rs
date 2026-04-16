use crate::dialect::Dialect;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
pub struct LongMemEvalMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LongMemEvalItem {
    pub question_id: String,
    pub question: String,
    pub answer: serde_json::Value,
    pub answer_session_ids: Vec<String>,
    pub haystack_session_ids: Vec<String>,
    pub haystack_sessions: Vec<Vec<LongMemEvalMessage>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub ndcg_at_10: f64,
    pub total_time_secs: f64,
    pub avg_ms_per_query: f64,
}

pub async fn run_longmemeval(path: &Path, mode: &str) -> Result<BenchmarkResult> {
    let content = std::fs::read_to_string(path)?;
    let items: Vec<LongMemEvalItem> = serde_json::from_str(&content)?;

    let mut total_recall_5 = 0;
    let mut total_recall_10 = 0;
    let mut total_ndcg_10 = 0.0;
    let count = items.len();

    let start_all = Instant::now();

    println!("Evaluating {} questions in mode '{}'...", count, mode);

    // 0. The shared embedder is automatically cached via the global EmbedderFactory
    // inside VectorStorage::new.

    let dialect = Dialect::new(None, None);

    for item in &items {
        // 1. Create temporary directory for isolated benchmarking
        let temp_dir = tempfile::tempdir()?;
        let db_path = temp_dir.path().join("bench.db");
        let index_path = temp_dir.path().join("bench.index");

        let mut storage = crate::vector_storage::VectorStorage::new(&db_path, &index_path)?;

        // Map indexed i64 IDs back to their original string IDs from the dataset
        let mut id_map = HashMap::new();

        // 2. Index the haystack
        for (idx, session) in item.haystack_sessions.iter().enumerate() {
            let session_id = &item.haystack_session_ids[idx];
            let mut full_text = String::new();
            for msg in session {
                full_text.push_str(&msg.content);
                full_text.push(' ');
            }

            let final_text = if mode == "aaak" {
                dialect.compress(&full_text, None)
            } else {
                full_text
            };

            // Noise Mitigation: Strip JSON metadata line before indexing to avoid vector search interference
            let indexed_text = if final_text.contains("\nJSON:{") {
                final_text
                    .split('\n')
                    .filter(|line| !line.starts_with("JSON:{"))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                final_text
            };

            let row_id = storage.add_memory(
                &indexed_text,
                "bench",
                "haystack",
                None, // source_file
                None, // temporal
            )?;

            id_map.insert(row_id, session_id.clone());
        }

        // 3. Search (Sync)
        let results = storage.search(&item.question, 10)?;

        // 4. Score
        let mut found_5 = false;
        let mut found_10 = false;
        let mut dcg = 0.0;

        for (rank, res) in results.iter().enumerate() {
            if let Some(orig_id) = id_map.get(&res.id) {
                let is_match = item.answer_session_ids.contains(orig_id);
                if is_match {
                    if rank < 5 {
                        found_5 = true;
                    }
                    if rank < 10 {
                        found_10 = true;
                    }

                    let rel = 1.0;
                    dcg += rel / (rank as f64 + 2.0).log2();
                }
            }
        }

        if found_5 {
            total_recall_5 += 1;
        }
        if found_10 {
            total_recall_10 += 1;
        }

        // IDEAL DCG calculation
        let num_correct = item.answer_session_ids.len();
        if num_correct > 0 {
            let mut idcg = 0.0;
            for i in 0..std::cmp::min(num_correct, 10) {
                idcg += 1.0 / (i as f64 + 2.0).log2();
            }
            if idcg > 0.0 {
                total_ndcg_10 += dcg / idcg;
            }
        }
    }

    let elapsed = start_all.elapsed();

    Ok(BenchmarkResult {
        recall_at_5: total_recall_5 as f64 / count as f64,
        recall_at_10: total_recall_10 as f64 / count as f64,
        ndcg_at_10: total_ndcg_10 / count as f64,
        total_time_secs: elapsed.as_secs_f64(),
        avg_ms_per_query: (elapsed.as_millis() as f64) / (count as f64),
    })
}
