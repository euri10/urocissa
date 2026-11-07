//! HTTP contract (v1). The sidecar is internal-only; it has no authentication
//! and must never be routed publicly.
//!
//! - `GET  /healthz`        → `{"status":"ok"}` once models are loaded.
//! - `GET  /v1/info`        → [`ModelInfo`].
//! - `POST /v1/embed/image` → raw image bytes (JPEG, PNG or WebP) in the body;
//!   returns `{"model_id", "embedding": [f32; dim]}`.
//! - `POST /v1/embed/text`  → `{"texts": ["…", …]}` (1–64 non-blank texts);
//!   returns `{"model_id", "embeddings": [[f32; dim], …]}` in input order.
//!
//! Errors are `{"error": "…"}` with 400 (bad input), 413 (body too large) or
//! 500 (inference failure). All embeddings are L2-normalized.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::embedder::{EmbedError, Embedder};
use crate::preprocess::{PreprocessError, decode_image};

pub const MAX_TEXTS_PER_REQUEST: usize = 64;
pub const MAX_TEXT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_image_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_image_bytes: 32 * 1024 * 1024,
        }
    }
}

pub fn router(embedder: Arc<dyn Embedder>, limits: Limits) -> Router {
    Router::new()
        .route("/healthz", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/v1/info", get(info))
        .route(
            "/v1/embed/image",
            post(embed_image).layer(DefaultBodyLimit::max(limits.max_image_bytes)),
        )
        .route("/v1/embed/text", post(embed_text))
        .with_state(embedder)
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(message) => {
                log::error!("{message}");
                (StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<EmbedError> for ApiError {
    fn from(error: EmbedError) -> Self {
        match error {
            EmbedError::Preprocess(PreprocessError::UndecodableImage(message)) => {
                Self::BadRequest(format!("image could not be decoded: {message}"))
            }
            other => Self::Internal(other.to_string()),
        }
    }
}

async fn info(State(embedder): State<Arc<dyn Embedder>>) -> impl IntoResponse {
    Json(embedder.info().clone())
}

#[derive(Serialize)]
struct ImageEmbedding {
    model_id: String,
    embedding: Vec<f32>,
}

async fn embed_image(
    State(embedder): State<Arc<dyn Embedder>>,
    body: Bytes,
) -> Result<Json<ImageEmbedding>, ApiError> {
    if body.is_empty() {
        return Err(ApiError::BadRequest(
            "request body must contain an image".into(),
        ));
    }
    let worker = Arc::clone(&embedder);
    let embedding = tokio::task::spawn_blocking(move || {
        let image = decode_image(&body).map_err(EmbedError::from)?;
        worker.embed_image(&image)
    })
    .await
    .map_err(|error| ApiError::Internal(format!("inference task failed: {error}")))??;
    Ok(Json(ImageEmbedding {
        model_id: embedder.info().model_id.clone(),
        embedding,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextRequest {
    texts: Vec<String>,
}

#[derive(Serialize)]
struct TextEmbeddings {
    model_id: String,
    embeddings: Vec<Vec<f32>>,
}

async fn embed_text(
    State(embedder): State<Arc<dyn Embedder>>,
    request: Result<Json<TextRequest>, JsonRejection>,
) -> Result<Json<TextEmbeddings>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    validate_texts(&request.texts)?;
    let worker = Arc::clone(&embedder);
    let embeddings = tokio::task::spawn_blocking(move || worker.embed_texts(&request.texts))
        .await
        .map_err(|error| ApiError::Internal(format!("inference task failed: {error}")))??;
    Ok(Json(TextEmbeddings {
        model_id: embedder.info().model_id.clone(),
        embeddings,
    }))
}

fn validate_texts(texts: &[String]) -> Result<(), ApiError> {
    if texts.is_empty() || texts.len() > MAX_TEXTS_PER_REQUEST {
        return Err(ApiError::BadRequest(format!(
            "texts must contain between 1 and {MAX_TEXTS_PER_REQUEST} entries"
        )));
    }
    if let Some(index) = texts.iter().position(|text| text.trim().is_empty()) {
        return Err(ApiError::BadRequest(format!("texts[{index}] is blank")));
    }
    if let Some(index) = texts
        .iter()
        .position(|text| text.chars().count() > MAX_TEXT_CHARS)
    {
        return Err(ApiError::BadRequest(format!(
            "texts[{index}] exceeds {MAX_TEXT_CHARS} characters"
        )));
    }
    Ok(())
}
