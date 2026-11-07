//! Helpers for tests that need an exported model (`TAGGER_MODEL_DIR`).

#![allow(dead_code)]

use std::path::PathBuf;

use gallery_tagger::openvino_backend::{BackendConfig, OpenVinoEmbedder};
use serde::Deserialize;

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn model_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("TAGGER_MODEL_DIR").expect("set TAGGER_MODEL_DIR to an exported model"),
    )
}

pub fn backend() -> BackendConfig {
    let devices = std::env::var("TAGGER_DEVICES").unwrap_or_else(|_| "CPU".into());
    BackendConfig {
        devices: devices
            .split(',')
            .map(|d| d.trim().to_uppercase())
            .collect(),
        cpu_threads: std::env::var("TAGGER_CPU_THREADS")
            .ok()
            .map(|t| t.parse().unwrap()),
        cpu_precision: std::env::var("TAGGER_CPU_PRECISION").ok(),
        cache_dir: None,
    }
}

pub fn load() -> OpenVinoEmbedder {
    OpenVinoEmbedder::load(&model_dir(), &backend()).expect("loading model")
}

#[derive(Deserialize)]
pub struct Reference {
    pub variant: String,
    pub logit_scale: f32,
    pub logit_bias: f32,
    pub images: std::collections::BTreeMap<String, Vec<f32>>,
    pub texts: Vec<ReferenceText>,
}

#[derive(Deserialize)]
pub struct ReferenceText {
    pub text: String,
    pub input_ids: Vec<i64>,
    pub embedding: Vec<f32>,
}

pub fn reference(variant: &str) -> Reference {
    let path = fixtures().join(format!("reference-{variant}.json"));
    let reference: Reference = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(reference.variant, variant);
    reference
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm(a) * norm(b))
}
