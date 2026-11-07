//! HTTP contract tests against a fake embedder; no model or `OpenVINO` needed.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use gallery_tagger::{
    api::{self, Limits},
    embedder::{EmbedError, Embedder, ModelInfo},
};
use http_body_util::BodyExt;
use image::DynamicImage;
use serde_json::{Value, json};
use tower::ServiceExt;

const DIM: usize = 4;

struct FakeEmbedder {
    info: ModelInfo,
    fail: bool,
}

impl FakeEmbedder {
    fn new(fail: bool) -> Self {
        Self {
            info: ModelInfo {
                model_id: "fake@0/fp32".into(),
                variant: "fake".into(),
                revision: "0".into(),
                precision: "fp32".into(),
                embedding_dim: DIM,
                image_size: 8,
                text_max_tokens: 64,
                logit_scale: 10.0,
                logit_bias: -5.0,
                device: "CPU".into(),
                runtime: "fake".into(),
                tagger_version: "test".into(),
            },
            fail,
        }
    }
}

impl Embedder for FakeEmbedder {
    fn info(&self) -> &ModelInfo {
        &self.info
    }

    fn embed_image(&self, image: &DynamicImage) -> Result<Vec<f32>, EmbedError> {
        if self.fail {
            return Err(EmbedError::Inference("device lost".into()));
        }
        let mut vector = vec![0.0; DIM];
        vector[(image.width() as usize) % DIM] = 1.0;
        Ok(vector)
    }

    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if self.fail {
            return Err(EmbedError::Inference("device lost".into()));
        }
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0; DIM];
                vector[text.len() % DIM] = 1.0;
                vector
            })
            .collect())
    }
}

fn app(fail: bool) -> axum::Router {
    api::router(
        Arc::new(FakeEmbedder::new(fail)),
        Limits {
            max_image_bytes: 4096,
        },
    )
}

async fn call(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).expect("API response must be JSON");
    (status, body)
}

fn post(uri: &str, content_type: &str, body: impl Into<Body>) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(body.into())
        .unwrap()
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    DynamicImage::new_rgb8(width, height)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

#[tokio::test]
async fn health_and_info() {
    let (status, body) = call(
        app(false),
        Request::get("/healthz").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!((status, body), (StatusCode::OK, json!({"status": "ok"})));

    let (status, body) = call(
        app(false),
        Request::get("/v1/info").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for key in [
        "model_id",
        "variant",
        "revision",
        "precision",
        "embedding_dim",
        "image_size",
        "text_max_tokens",
        "logit_scale",
        "logit_bias",
        "device",
        "runtime",
        "tagger_version",
    ] {
        assert!(body.get(key).is_some(), "info is missing {key}");
    }
    assert_eq!(body["model_id"], "fake@0/fp32");
}

#[tokio::test]
async fn embeds_an_image() {
    let (status, body) = call(app(false), post("/v1/embed/image", "image/png", png(5, 3))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["model_id"], "fake@0/fp32");
    assert_eq!(body["embedding"], json!([0.0, 1.0, 0.0, 0.0]));
}

#[tokio::test]
async fn rejects_bad_images() {
    for body in [Vec::new(), b"not an image at all".to_vec()] {
        let (status, json) = call(app(false), post("/v1/embed/image", "image/jpeg", body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].is_string());
    }
}

#[tokio::test]
async fn rejects_oversized_images_with_json() {
    let oversized = vec![0_u8; 5000];
    let (status, body) = call(app(false), post("/v1/embed/image", "image/jpeg", oversized)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn embeds_texts_in_order() {
    let request = json!({"texts": ["a", "bb", "ccc"]}).to_string();
    let (status, body) = call(
        app(false),
        post("/v1/embed/text", "application/json", request),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["embeddings"],
        json!([
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0]
        ])
    );
}

#[tokio::test]
async fn rejects_bad_text_requests() {
    let too_many: Vec<String> = (0..=api::MAX_TEXTS_PER_REQUEST)
        .map(|i| i.to_string())
        .collect();
    let too_long = "x".repeat(api::MAX_TEXT_CHARS + 1);
    let cases = [
        json!({"texts": []}).to_string(),
        json!({"texts": ["ok", "   "]}).to_string(),
        json!({"texts": too_many}).to_string(),
        json!({"texts": [too_long]}).to_string(),
        json!({"texts": ["ok"], "extra": 1}).to_string(),
        json!({"text": "ok"}).to_string(),
        "{not json".to_owned(),
    ];
    for case in cases {
        let (status, body) = call(
            app(false),
            post("/v1/embed/text", "application/json", case.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {case}");
        assert!(body["error"].is_string(), "case {case}");
    }
}

#[tokio::test]
async fn inference_failures_are_server_errors() {
    let (status, body) = call(app(true), post("/v1/embed/image", "image/png", png(2, 2))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body["error"].as_str().unwrap().contains("device lost"));
    let request = json!({"texts": ["a"]}).to_string();
    let (status, _) = call(
        app(true),
        post("/v1/embed/text", "application/json", request),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
