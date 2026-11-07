//! The embedding contract served over HTTP, independent of the inference runtime.

use image::DynamicImage;
use serde::Serialize;
use thiserror::Error;

use crate::preprocess::PreprocessError;

/// Everything a client needs to interpret and compare embeddings.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelInfo {
    /// Changes whenever weights or export precision change. Clients must
    /// re-embed everything when it differs from what they stored.
    pub model_id: String,
    pub variant: String,
    pub revision: String,
    pub precision: String,
    pub embedding_dim: usize,
    pub image_size: u32,
    pub text_max_tokens: usize,
    /// Already exponentiated: probability = sigmoid(cos * `logit_scale` + `logit_bias`).
    pub logit_scale: f32,
    pub logit_bias: f32,
    /// Devices actually executing inference, as reported by the runtime.
    pub device: String,
    pub runtime: String,
    pub tagger_version: String,
}

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error(transparent)]
    Preprocess(#[from] PreprocessError),
    #[error("inference failed: {0}")]
    Inference(String),
}

/// Turns images and texts into L2-normalized embeddings. Stateless: identical
/// inputs always yield identical outputs for the same `model_id`.
pub trait Embedder: Send + Sync + 'static {
    fn info(&self) -> &ModelInfo;
    fn embed_image(&self, image: &DynamicImage) -> Result<Vec<f32>, EmbedError>;
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;
}

/// Scale a vector to unit length; zero vectors are returned unchanged.
#[must_use]
pub fn l2_normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

#[cfg(test)]
mod tests {
    use super::l2_normalize;

    #[test]
    fn normalizes_to_unit_length() {
        let vector = l2_normalize(vec![3.0, 4.0]);
        assert!((vector[0] - 0.6).abs() < 1e-6 && (vector[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn zero_vector_is_unchanged() {
        assert_eq!(l2_normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }
}
