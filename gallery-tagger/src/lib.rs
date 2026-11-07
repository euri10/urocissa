//! `gallery-tagger`: a stateless SigLIP 2 embedding sidecar for Urocissa.
//!
//! It turns images and texts into L2-normalized embeddings and reports which
//! model and device produced them. It holds no vocabulary, tags or user state;
//! all auto-tagging logic lives in the gallery. See [`api`] for the contract.

// Internal service crate: product names (OpenVINO, SigLIP) need no backticks
// and errors are documented on the types rather than per function.
#![allow(clippy::doc_markdown, clippy::missing_errors_doc)]

pub mod api;
pub mod config;
pub mod embedder;
pub mod manifest;
pub mod openvino_backend;
pub mod preprocess;
