//! The exported model's manifest, written by `tools/export_openvino.py`.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const VISION_MODEL: &str = "vision";
pub const TEXT_MODEL: &str = "text";
pub const TOKENIZER_FILE: &str = "tokenizer.json";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Identifies the exact weights and export precision. Embeddings from
    /// different model IDs are not comparable.
    pub model_id: String,
    pub variant: String,
    pub revision: String,
    pub precision: String,
    pub embedding_dim: usize,
    pub image_size: u32,
    pub image_mean: [f32; 3],
    pub image_std: [f32; 3],
    pub rescale_factor: f32,
    pub text_max_tokens: usize,
    pub text_lowercase: bool,
    /// Already exponentiated: probability = sigmoid(cos * `logit_scale` + `logit_bias`).
    pub logit_scale: f32,
    pub logit_bias: f32,
}

impl Manifest {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let path = model_dir.join(MANIFEST_FILE);
        let raw = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let manifest: Self =
            serde_json::from_slice(&raw).with_context(|| format!("parsing {}", path.display()))?;
        ensure!(
            manifest.schema_version == 1,
            "unsupported manifest schema {}",
            manifest.schema_version
        );
        ensure!(manifest.embedding_dim > 0, "embedding_dim must be positive");
        ensure!(manifest.image_size > 0, "image_size must be positive");
        ensure!(
            manifest.text_max_tokens > 1,
            "text_max_tokens must exceed 1"
        );
        ensure!(
            manifest.image_std.iter().all(|value| *value > 0.0),
            "image_std must be positive"
        );
        Ok(manifest)
    }
}
