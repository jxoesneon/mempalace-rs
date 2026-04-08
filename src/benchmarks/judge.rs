use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JudgeResponse {
    pub answer: String,
    pub score: f64,
    pub reasoning: String,
}

#[async_trait]
pub trait Judge: Send + Sync {
    /// Compare the assistant's answer against the ground truth
    async fn evaluate(
        &self,
        question: &str,
        assistant_answer: &str,
        ground_truth: &str,
    ) -> Result<JudgeResponse>;

    /// Evaluate a 'nugget' for BEAM (atomic semantic unit)
    async fn evaluate_nugget(&self, assistant_answer: &str, nugget: &str) -> Result<f64>;
}

pub struct MockJudge;

#[async_trait]
impl Judge for MockJudge {
    async fn evaluate(&self, _q: &str, _aa: &str, _gt: &str) -> Result<JudgeResponse> {
        Ok(JudgeResponse {
            answer: "Mock Answer".to_string(),
            score: 1.0,
            reasoning: "Mock Reasoning".to_string(),
        })
    }

    async fn evaluate_nugget(&self, _aa: &str, _n: &str) -> Result<f64> {
        Ok(1.0)
    }
}
