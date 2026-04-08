use crate::benchmarks::judge::Judge;
use crate::benchmarks::{Benchmark, BenchmarkResult};
use crate::vector_storage::VectorStorage;
use crate::MemoryRecord;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;

/// BEAM (Benchmark for Evaluating Agentic Memory)
pub struct BeamBenchmark {
    pub judge: Box<dyn Judge>,
}

#[derive(Debug, Clone)]
pub struct BeamScenario {
    pub name: String,
    pub nuggets: Vec<String>,
    pub dialogue: Vec<String>,
    pub probe: String,
    pub follow_up_expected: bool,
}

#[async_trait]
impl Benchmark for BeamBenchmark {
    fn name(&self) -> &str {
        "BEAM"
    }

    fn description(&self) -> &str {
        "Benchmark for Evaluating Agentic Memory (Nuggets, Coherence, Follow-Up)"
    }

    async fn run(&self, storage: &mut VectorStorage) -> Result<BenchmarkResult> {
        let start = Instant::now();
        let scenarios = self.load_default_scenarios();
        let mut total_score = 0.0;
        let mut total_scenarios = 0;
        let mut metadata = HashMap::new();

        for scenario in scenarios {
            let wing = format!("beam_{}", scenario.name);
            let room = "eval_chamber";

            for (i, line) in scenario.dialogue.iter().enumerate() {
                storage.add_memory(
                    line,
                    &wing,
                    room,
                    Some(&format!("dialogue_{}.txt", i)),
                    None,
                )?;
            }

            let retrieved = storage.search_room(&scenario.probe, &wing, room, 10, None)?;

            let assistant_answer = retrieved
                .iter()
                .map(|m| m.text_content.as_str())
                .collect::<Vec<_>>()
                .join("\n");

            let mut scenario_nugget_score = 0.0;
            for nugget in &scenario.nuggets {
                let score = self
                    .judge
                    .evaluate_nugget(&assistant_answer, nugget)
                    .await?;
                scenario_nugget_score += score;
            }

            if !scenario.nuggets.is_empty() {
                scenario_nugget_score /= scenario.nuggets.len() as f64;
            }

            let follow_up_score = self
                .simulate_follow_up_detection(&scenario, &retrieved)
                .await?;
            let coherence_score = self.evaluate_coherence(&assistant_answer).await?;

            let final_scenario_score =
                (scenario_nugget_score * 0.5) + (follow_up_score * 0.3) + (coherence_score * 0.2);

            metadata.insert(
                format!("{}_score", scenario.name),
                format!("{:.2}", final_scenario_score),
            );

            total_score += final_scenario_score;
            total_scenarios += 1;
        }

        let avg_score = if total_scenarios > 0 {
            total_score / total_scenarios as f64
        } else {
            0.0
        };

        Ok(BenchmarkResult {
            name: self.name().to_string(),
            score: avg_score,
            metric_name: "BEAM-Aggregate".to_string(),
            latency_ms: start.elapsed().as_millis() as f64,
            tokens_used: 0,
            metadata,
        })
    }
}

impl BeamBenchmark {
    pub fn new(judge: Box<dyn Judge>) -> Self {
        Self { judge }
    }

    async fn simulate_follow_up_detection(
        &self,
        scenario: &BeamScenario,
        _retrieved: &[MemoryRecord],
    ) -> Result<f64> {
        if !scenario.follow_up_expected {
            return Ok(1.0);
        }

        let response = self
            .judge
            .evaluate(
                &scenario.probe,
                "I cannot answer this definitively without more information.",
                &scenario.nuggets.join(", "),
            )
            .await?;

        if response.score > 0.7 {
            Ok(1.0)
        } else {
            Ok(0.0)
        }
    }

    async fn evaluate_coherence(&self, retrieved_text: &str) -> Result<f64> {
        let lines: Vec<&str> = retrieved_text.lines().collect();
        if lines.len() >= 3 {
            Ok(1.0)
        } else if !lines.is_empty() {
            Ok(0.5)
        } else {
            Ok(0.0)
        }
    }

    fn load_default_scenarios(&self) -> Vec<BeamScenario> {
        vec![
            BeamScenario {
                name: "TravelPlanner".to_string(),
                dialogue: vec![
                    "I am planning a trip to Tokyo in October.".to_string(),
                    "I love sushi but I have a severe shellfish allergy.".to_string(),
                    "I want to see the Ghibli Museum, but tickets are hard to get.".to_string(),
                ],
                nuggets: vec![
                    "Tokyo trip in October".to_string(),
                    "Shellfish allergy".to_string(),
                    "Ghibli Museum interest".to_string(),
                ],
                probe: "What are my travel constraints?".to_string(),
                follow_up_expected: false,
            },
            BeamScenario {
                name: "ProjectDeadline".to_string(),
                dialogue: vec![
                    "We need to finish the migration by Friday.".to_string(),
                    "John is the only one who knows the database password.".to_string(),
                    "John is on vacation until next Tuesday.".to_string(),
                ],
                nuggets: vec![
                    "Migration deadline Friday".to_string(),
                    "John knows password".to_string(),
                    "John away until Tuesday".to_string(),
                ],
                probe: "Can we finish the migration on time?".to_string(),
                follow_up_expected: true,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmarks::judge::MockJudge;
    use crate::vector_storage::VectorStorage;

    #[tokio::test]
    async fn test_beam_benchmark_run() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let db_path = temp_dir.path().join("test_beam.db");
        let index_path = temp_dir.path().join("test_beam.idx");

        let mut storage = VectorStorage::new(db_path, index_path)?;
        let benchmark = BeamBenchmark::new(Box::new(MockJudge));

        let result = benchmark.run(&mut storage).await?;

        assert_eq!(result.name, "BEAM");
        assert!(result.score >= 0.0 && result.score <= 1.0);
        assert!(result.metadata.contains_key("TravelPlanner_score"));
        assert!(result.metadata.contains_key("ProjectDeadline_score"));

        Ok(())
    }
}
