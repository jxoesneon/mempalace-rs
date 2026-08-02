//! Embedding utilities for the memory palace.
//!
//! Provides a thin wrapper around the embedder factory plus vector math helpers
//! (cosine similarity, top-k search, normalization). Production code uses the
//! fastembed singleton, while tests can plug in a fake embedder via the
//! `Embedder` trait.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Something that can turn text into a vector.
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Production embedder backed by the global fastembed singleton.
#[derive(Debug, Clone, Copy, Default)]
pub struct FastEmbedder;

impl Embedder for FastEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embedder = crate::embedder_factory::EmbedderFactory::get_embedder()
            .context("failed to get embedder")?;
        let mut embedder = embedder.lock().expect("embedder mutex poisoned");
        let mut out = embedder
            .embed(vec![text.to_string()], None)
            .context("embedding failed")?;
        out.pop().context("empty embedding result")
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let embedder = crate::embedder_factory::EmbedderFactory::get_embedder()
            .context("failed to get embedder")?;
        let mut embedder = embedder.lock().expect("embedder mutex poisoned");
        embedder
            .embed(texts.to_vec(), None)
            .context("batch embedding failed")
    }
}

/// A single embedding record with optional metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Embedding {
    pub text: String,
    pub vector: Vec<f32>,
}

impl Embedding {
    pub fn new(text: impl Into<String>, vector: Vec<f32>) -> Self {
        Self {
            text: text.into(),
            vector,
        }
    }

    /// L2-normalize the vector in place.
    pub fn normalize(&mut self) {
        normalize_vector(&mut self.vector);
    }

    /// Cosine similarity to another vector.
    pub fn cosine_similarity(&self, other: &[f32]) -> f32 {
        cosine_similarity(&self.vector, other)
    }
}

/// Dot product of two vectors with an unrolled loop that auto-vectorizes well
/// on modern CPUs without requiring target-specific intrinsics.
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len();
    let mut sum0 = 0.0f32;
    let mut sum1 = 0.0f32;
    let mut sum2 = 0.0f32;
    let mut sum3 = 0.0f32;
    let mut i = 0;
    while i + 4 <= len {
        sum0 += a[i] * b[i];
        sum1 += a[i + 1] * b[i + 1];
        sum2 += a[i + 2] * b[i + 2];
        sum3 += a[i + 3] * b[i + 3];
        i += 4;
    }
    let mut sum = sum0 + sum1 + sum2 + sum3;
    while i < len {
        sum += a[i] * b[i];
        i += 1;
    }
    sum
}

/// L2 norm of a vector with an unrolled loop for auto-vectorization.
fn vector_norm(v: &[f32]) -> f32 {
    let len = v.len();
    let mut sum0 = 0.0f32;
    let mut sum1 = 0.0f32;
    let mut sum2 = 0.0f32;
    let mut sum3 = 0.0f32;
    let mut i = 0;
    while i + 4 <= len {
        let x0 = v[i];
        let x1 = v[i + 1];
        let x2 = v[i + 2];
        let x3 = v[i + 3];
        sum0 += x0 * x0;
        sum1 += x1 * x1;
        sum2 += x2 * x2;
        sum3 += x3 * x3;
        i += 4;
    }
    let mut sum = sum0 + sum1 + sum2 + sum3;
    while i < len {
        sum += v[i] * v[i];
        i += 1;
    }
    sum.sqrt()
}

/// Cosine similarity between two vectors using a cached query norm.
fn cosine_similarity_with_query_norm(query: &[f32], query_norm: f32, other: &[f32]) -> f32 {
    if query.len() != other.len() || query.is_empty() {
        return 0.0;
    }
    let dot = dot_product(query, other);
    let norm_other = vector_norm(other);
    if query_norm == 0.0 || norm_other == 0.0 {
        return 0.0;
    }
    dot / (query_norm * norm_other)
}

/// Cosine similarity between two vectors. Vectors are normalized internally so
/// callers do not need to pre-normalize.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let norm_a = vector_norm(a);
    cosine_similarity_with_query_norm(a, norm_a, b)
}

/// Cosine similarity assuming both vectors are already L2-normalized.
/// This avoids recomputing norms and is useful for repeated comparisons.
pub fn cosine_similarity_normalized(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    dot_product(a, b)
}

/// Euclidean distance between two vectors.
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::INFINITY;
    }
    let len = a.len();
    let mut sum0 = 0.0f32;
    let mut sum1 = 0.0f32;
    let mut sum2 = 0.0f32;
    let mut sum3 = 0.0f32;
    let mut i = 0;
    while i + 4 <= len {
        let d0 = a[i] - b[i];
        let d1 = a[i + 1] - b[i + 1];
        let d2 = a[i + 2] - b[i + 2];
        let d3 = a[i + 3] - b[i + 3];
        sum0 += d0 * d0;
        sum1 += d1 * d1;
        sum2 += d2 * d2;
        sum3 += d3 * d3;
        i += 4;
    }
    let mut sum = sum0 + sum1 + sum2 + sum3;
    while i < len {
        let d = a[i] - b[i];
        sum += d * d;
        i += 1;
    }
    sum.sqrt()
}

/// L2-normalize a vector in place.
pub fn normalize_vector(v: &mut [f32]) {
    let norm = vector_norm(v);
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

/// Find the top-k indices most similar to `query` from a slice of vectors.
/// The query norm is computed once and reused for every candidate.
pub fn top_k_similar(query: &[f32], vectors: &[Vec<f32>], k: usize) -> Vec<(usize, f32)> {
    let k = k.min(vectors.len());
    if k == 0 || query.is_empty() {
        return Vec::new();
    }
    let query_norm = vector_norm(query);
    if query_norm == 0.0 {
        return Vec::new();
    }
    let mut scored: Vec<(usize, f32)> = vectors
        .iter()
        .enumerate()
        .map(|(idx, vec)| {
            (
                idx,
                cosine_similarity_with_query_norm(query, query_norm, vec),
            )
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

/// Find the top-k indices most similar to a pre-normalized query from a slice of
/// pre-normalized vectors. This skips all norm calculations and uses a fast dot
/// product.
pub fn top_k_similar_normalized(
    query: &[f32],
    vectors: &[Vec<f32>],
    k: usize,
) -> Vec<(usize, f32)> {
    let k = k.min(vectors.len());
    if k == 0 || query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, f32)> = vectors
        .iter()
        .enumerate()
        .map(|(idx, vec)| (idx, cosine_similarity_normalized(query, vec)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

/// Embed a batch of texts and normalize the resulting vectors.
pub fn embed_and_normalize<E: Embedder>(embedder: &E, texts: &[String]) -> Result<Vec<Embedding>> {
    let vectors = embedder.embed_batch(texts)?;
    let mut embeddings: Vec<Embedding> = texts
        .iter()
        .zip(vectors)
        .map(|(t, v)| Embedding::new(t.clone(), v))
        .collect();
    for e in embeddings.iter_mut() {
        e.normalize();
    }
    Ok(embeddings)
}

/// Compute the centroid of a set of vectors.
pub fn centroid(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    if vectors.is_empty() {
        return None;
    }
    let dim = vectors[0].len();
    if !vectors.iter().all(|v| v.len() == dim) {
        return None;
    }
    let n = vectors.len() as f32;
    let mut out = vec![0.0f32; dim];
    for v in vectors {
        for (i, x) in v.iter().enumerate() {
            out[i] += x / n;
        }
    }
    Some(out)
}

/// Set the active embedder model for the palace.
///
/// This is a placeholder that writes the model name to a small JSON file in the
/// config directory. The actual embedder switching is handled by the upstream
/// embedder factory.
pub fn set_embedder(config_dir: impl AsRef<std::path::Path>, model: &str) -> Result<()> {
    let path = config_dir.as_ref().join("embedder.json");
    let content = serde_json::json!({ "model": model });
    std::fs::write(&path, serde_json::to_string_pretty(&content)?)
        .with_context(|| format!("failed to write embedder config to {path:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEmbedder;

    impl Embedder for FakeEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            Ok(vec![
                text.len() as f32,
                text.chars().filter(|c| c.is_alphabetic()).count() as f32,
            ])
        }

        fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            texts.iter().map(|t| self.embed(t)).collect()
        }
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0f32, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty_or_mismatch() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![1.0f32, 2.0, 3.0];
        let b = vec![4.0f32, 5.0, 6.0];
        assert!((euclidean_distance(&a, &b) - 5.196152).abs() < 1e-5);
    }

    #[test]
    fn test_normalize_vector() {
        let mut v = vec![3.0f32, 4.0];
        normalize_vector(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_embedding_normalize() {
        let mut e = Embedding::new("hello", vec![3.0, 4.0]);
        e.normalize();
        assert!((e.cosine_similarity(&[0.6, 0.8]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_top_k_similar() {
        let query = vec![1.0f32, 0.0];
        let vectors = vec![
            vec![1.0, 0.0],     // 1.0
            vec![0.0, 1.0],     // 0.0
            vec![0.707, 0.707], // ~0.707
            vec![-1.0, 0.0],    // -1.0
        ];
        let top = top_k_similar(&query, &vectors, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, 0);
        assert!(top[0].1 > 0.99);
        assert_eq!(top[1].0, 2);
    }

    #[test]
    fn test_top_k_similar_empty() {
        assert!(top_k_similar(&[1.0], &[], 3).is_empty());
    }

    #[test]
    fn test_embed_and_normalize_fake() {
        let texts = vec!["ab".to_string(), "abc".to_string()];
        let out = embed_and_normalize(&FakeEmbedder, &texts).unwrap();
        assert_eq!(out.len(), 2);
        assert!((out[0].vector.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_centroid() {
        let v = vec![vec![1.0f32, 2.0], vec![3.0f32, 4.0]];
        let c = centroid(&v).unwrap();
        assert!((c[0] - 2.0).abs() < 1e-6);
        assert!((c[1] - 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_centroid_empty_or_mismatch() {
        assert!(centroid(&[]).is_none());
        assert!(centroid(&[vec![1.0], vec![1.0, 2.0]]).is_none());
    }

    #[test]
    fn test_optimized_vector_math_matches_naive() {
        // Verify that the unrolled/dot-product optimizations agree with the
        // naive scalar implementations for odd and even lengths, including
        // values that exercise SIMD rounding.
        let a: Vec<f32> = (1..=17).map(|i| i as f32 * 0.13).collect();
        let b: Vec<f32> = (1..=17).map(|i| i as f32 * -0.07).collect();

        let naive_dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
        let opt_dot = dot_product(&a, &b);
        assert!((naive_dot - opt_dot).abs() < 1e-5, "dot_product mismatch");

        let naive_norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let opt_norm_a = vector_norm(&a);
        assert!(
            (naive_norm_a - opt_norm_a).abs() < 1e-5,
            "vector_norm mismatch"
        );

        let naive_cos = naive_dot / (naive_norm_a * vector_norm(&b));
        let opt_cos = cosine_similarity(&a, &b);
        assert!(
            (naive_cos - opt_cos).abs() < 1e-5,
            "cosine_similarity mismatch"
        );

        let naive_euclidean = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt();
        let opt_euclidean = euclidean_distance(&a, &b);
        assert!(
            (naive_euclidean - opt_euclidean).abs() < 1e-5,
            "euclidean_distance mismatch"
        );
    }

    #[test]
    fn test_cosine_similarity_normalized() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!(cosine_similarity_normalized(&a, &b).abs() < 1e-6);

        let c = vec![0.6f32, 0.8];
        let d = vec![0.6f32, 0.8];
        assert!((cosine_similarity_normalized(&c, &d) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_top_k_similar_normalized() {
        let query = vec![1.0f32, 0.0];
        let vectors = vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![0.707, 0.707]];
        let top = top_k_similar_normalized(&query, &vectors, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, 0);
        assert!(top[0].1 > 0.99);
        assert_eq!(top[1].0, 2);
    }

    #[test]
    fn test_set_embedder() {
        let dir = tempfile::tempdir().unwrap();
        set_embedder(dir.path(), "all-MiniLM-L6-v2").unwrap();
        let path = dir.path().join("embedder.json");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("all-MiniLM-L6-v2"));
    }
}
