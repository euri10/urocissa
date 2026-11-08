//! The stateless sidecar's v1 HTTP boundary. No gallery state crosses this interface.
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model_id: String,
    pub embedding_dim: usize,
    pub device: String,
    #[serde(default)]
    pub logit_scale: Option<f32>,
    #[serde(default)]
    pub logit_bias: Option<f32>,
}

impl ModelInfo {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            match (self.logit_scale, self.logit_bias) {
                (None, None) => true,
                (Some(scale), Some(bias)) => scale.is_finite() && scale > 0.0 && bias.is_finite(),
                _ => false,
            },
            "invalid or incomplete tagger scoring metadata"
        );
        ensure!(
            !self.model_id.trim().is_empty(),
            "tagger returned an empty model ID"
        );
        ensure!(self.model_id.len() <= 512, "tagger model ID is too long");
        ensure!(
            (1..=8192).contains(&self.embedding_dim),
            "invalid embedding dimension"
        );
        ensure!(
            !self.device.trim().is_empty(),
            "tagger returned no active device"
        );
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct ImageEmbedding {
    pub model_id: String,
    pub embedding: Vec<f32>,
}

impl ImageEmbedding {
    pub fn validate(&self, model: &ModelInfo) -> Result<()> {
        ensure!(
            self.model_id == model.model_id,
            "tagger model changed during inference"
        );
        crate::operations::autotag::check_vector(&self.embedding, model.embedding_dim)
            .map_err(|error| anyhow::anyhow!("tagger returned an invalid embedding: {error}"))
    }
}

/// Blocking calls run on the embedding worker or a blocking search task.
pub trait Embedder: Send + Sync {
    fn info(&self) -> Result<ModelInfo>;
    fn embed_image(&self, jpeg: Vec<u8>) -> Result<ImageEmbedding>;
    fn embed_text(&self, _text: &str) -> Result<ImageEmbedding> {
        anyhow::bail!("text embeddings are unavailable")
    }
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<ImageEmbedding>> {
        texts.iter().map(|text| self.embed_text(text)).collect()
    }
}

pub struct HttpEmbedder {
    base: Url,
    client: Client,
}

impl HttpEmbedder {
    pub fn new(url: &str) -> Result<Self> {
        Self::with_timeout(url, Duration::from_secs(10))
    }

    fn with_timeout(url: &str, timeout: Duration) -> Result<Self> {
        let base = Url::parse(&format!("{}/", url.trim().trim_end_matches('/')))
            .context("invalid tagger URL")?;
        ensure!(
            matches!(base.scheme(), "http" | "https") && base.host_str().is_some(),
            "tagger URL must be HTTP or HTTPS"
        );
        ensure!(
            base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none(),
            "tagger URL must not contain credentials, a query or a fragment"
        );
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3).min(timeout))
            .timeout(timeout)
            .redirect(Policy::none())
            .no_proxy()
            .build()?;
        Ok(Self { base, client })
    }
}

fn decode<T: serde::de::DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    use std::io::Read;
    // A broken sidecar must not allocate an unbounded response in the gallery.
    const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    response
        .error_for_status()?
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_RESPONSE,
        "tagger response is too large"
    );
    serde_json::from_slice(&bytes).context("invalid tagger response")
}

impl Embedder for HttpEmbedder {
    fn info(&self) -> Result<ModelInfo> {
        let info: ModelInfo = decode(self.client.get(self.base.join("v1/info")?).send()?)?;
        info.validate()?;
        Ok(info)
    }

    fn embed_image(&self, jpeg: Vec<u8>) -> Result<ImageEmbedding> {
        decode(
            self.client
                .post(self.base.join("v1/embed/image")?)
                .header("Content-Type", "image/jpeg")
                .body(jpeg)
                .send()?,
        )
    }

    fn embed_text(&self, text: &str) -> Result<ImageEmbedding> {
        Ok(self.embed_texts(&[text.to_owned()])?.remove(0))
    }

    fn embed_texts(&self, texts: &[String]) -> Result<Vec<ImageEmbedding>> {
        #[derive(Deserialize)]
        struct Response {
            model_id: String,
            embeddings: Vec<Vec<f32>>,
        }
        ensure!(
            !texts.is_empty() && texts.len() <= 64,
            "invalid text batch size"
        );
        let response: Response = decode(
            self.client
                .post(self.base.join("v1/embed/text")?)
                .json(&serde_json::json!({ "texts": texts }))
                .send()?,
        )?;
        ensure!(
            response.embeddings.len() == texts.len(),
            "wrong text embedding count"
        );
        Ok(response
            .embeddings
            .into_iter()
            .map(|embedding| ImageEmbedding {
                model_id: response.model_id.clone(),
                embedding,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
