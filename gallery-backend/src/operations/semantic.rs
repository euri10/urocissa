//! Owner search orchestration and short-lived, fixed semantic result sets.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arrayvec::ArrayString;
use serde::{Deserialize, Serialize};

use super::embedding::client::{Embedder, ImageEmbedding, ModelInfo};
use crate::{
    public::{
        error::{AppError, ErrorKind},
        structure::expression::Expression,
    },
    router::AppResult,
    storage::embeddings::EmbeddingStore,
};

const QUERY_TTL: Duration = Duration::from_mins(1);
const SELECTION_TTL: Duration = Duration::from_mins(5);
const MAX_QUERIES: usize = 32;
const MAX_SELECTIONS: usize = 64;
pub const MAX_RESULTS: usize = 200;

#[derive(Debug, PartialEq, Eq)]
pub enum SearchQuery {
    Text(String),
    Similar(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSummary {
    pub selection: String,
    pub indexed_photos: usize,
    pub pending_photos: usize,
}

#[derive(Clone)]
pub struct Selection {
    pub summary: SearchSummary,
    pub photos: Vec<(ArrayString<64>, u32)>,
    expression: Expression,
    accessed: Instant,
}

struct CachedQuery {
    text: String,
    model: ModelInfo,
    embedding: Vec<f32>,
    created: Instant,
}

pub struct SemanticSearch {
    embedder: Arc<dyn Embedder>,
    pub store: EmbeddingStore,
    queries: Mutex<VecDeque<CachedQuery>>,
    selections: Mutex<VecDeque<Selection>>,
}

impl SemanticSearch {
    pub fn new(embedder: Arc<dyn Embedder>, store: EmbeddingStore) -> Self {
        Self {
            embedder,
            store,
            queries: Mutex::new(VecDeque::new()),
            selections: Mutex::new(VecDeque::new()),
        }
    }

    pub fn query_vector(&self, text: &str) -> AppResult<ImageEmbedding> {
        let unavailable = |_| {
            AppError::new(
                ErrorKind::Unavailable,
                "Semantic search is unavailable: the tagger is offline or returned an invalid response. Other searches still work.",
            )
        };
        // Always probe, including cache hits: do not conceal an outage or a model change.
        let model = self.embedder.info().map_err(unavailable)?;
        model.validate().map_err(unavailable)?;
        let now = Instant::now();
        {
            let mut queries = self.queries.lock().unwrap();
            queries.retain(|entry| now.duration_since(entry.created) < QUERY_TTL);
            if let Some(entry) = queries.iter().find(|entry| {
                entry.text == text
                    && entry.model.model_id == model.model_id
                    && entry.model.embedding_dim == model.embedding_dim
            }) {
                return Ok(ImageEmbedding {
                    model_id: model.model_id,
                    embedding: entry.embedding.clone(),
                });
            }
        }
        let vector = self.embedder.embed_text(text).map_err(unavailable)?;
        vector.validate(&model).map_err(unavailable)?;
        let mut queries = self.queries.lock().unwrap();
        if queries.len() >= MAX_QUERIES {
            queries.pop_front();
        }
        queries.push_back(CachedQuery {
            text: text.to_owned(),
            model,
            embedding: vector.embedding.clone(),
            created: now,
        });
        Ok(vector)
    }

    pub fn remember(
        &self,
        expression: Expression,
        photos: Vec<(ArrayString<64>, u32)>,
        indexed_photos: usize,
        pending_photos: usize,
    ) -> Selection {
        let selection = Selection {
            summary: SearchSummary {
                selection: uuid::Uuid::new_v4().to_string(),
                indexed_photos,
                pending_photos,
            },
            photos,
            expression,
            accessed: Instant::now(),
        };
        let mut selections = self.selections.lock().unwrap();
        selections.retain(|entry| entry.accessed.elapsed() < SELECTION_TTL);
        if selections.len() >= MAX_SELECTIONS {
            selections.pop_front();
        }
        selections.push_back(selection.clone());
        selection
    }

    pub fn resume(&self, id: &str, expression: &Expression) -> AppResult<Selection> {
        let mut selections = self.selections.lock().unwrap();
        selections.retain(|entry| entry.accessed.elapsed() < SELECTION_TTL);
        let entry = selections
            .iter_mut()
            .find(|entry| entry.summary.selection == id && &entry.expression == expression)
            .ok_or_else(|| {
                AppError::new(
                    ErrorKind::Conflict,
                    "Semantic results expired or the filter changed. Run the search again.",
                )
            })?;
        entry.accessed = Instant::now();
        Ok(entry.clone())
    }
}

/// One positive text/image query can be combined with any existing filters using AND.
/// Reject ambiguous rankings under OR/NOT rather than silently broadening the collection.
pub fn split_expression(expression: &Expression) -> AppResult<(SearchQuery, Expression)> {
    fn extract(expression: &Expression, query: &mut Option<SearchQuery>) -> AppResult<Expression> {
        Ok(match expression {
            Expression::Semantic(value) | Expression::Similar(value) => {
                let value = value.trim();
                let ranked = if matches!(expression, Expression::Similar(_)) {
                    if value.len() != 64
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                    {
                        return Err(AppError::new(
                            ErrorKind::InvalidInput,
                            "Similar photos requires a photo ID (64 lowercase hexadecimal characters)",
                        ));
                    }
                    SearchQuery::Similar(value.to_owned())
                } else {
                    if value.is_empty() || value.chars().count() > 2000 {
                        return Err(AppError::new(
                            ErrorKind::InvalidInput,
                            "Semantic text must contain 1–2000 characters",
                        ));
                    }
                    SearchQuery::Text(value.to_owned())
                };
                if query.replace(ranked).is_some() {
                    return Err(AppError::new(
                        ErrorKind::InvalidInput,
                        "Use one semantic or similar expression per search",
                    ));
                }
                Expression::And(vec![])
            }
            Expression::And(children) => Expression::And(
                children
                    .iter()
                    .map(|child| extract(child, query))
                    .collect::<AppResult<_>>()?,
            ),
            expression if expression.contains_semantic() => {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Combine semantic or similar search with filters using AND, not OR or NOT",
                ));
            }
            expression => expression.clone(),
        })
    }
    let mut query = None;
    let filter = extract(expression, &mut query)?;
    let query = query.ok_or_else(|| {
        AppError::new(
            ErrorKind::InvalidInput,
            "Relevance sorting requires a semantic or similar expression",
        )
    })?;
    Ok((query, filter))
}

/// Cosine ranking of normalized vectors; ties are stable and never expand the result cap.
pub fn rank<T: Ord>(mut scores: Vec<(T, f32)>) -> Vec<T> {
    scores.retain(|(_, score)| score.is_finite() && *score > 0.0);
    scores.sort_unstable_by(|(left, a), (right, b)| b.total_cmp(a).then_with(|| left.cmp(right)));
    let cutoff = scores.first().map_or(0.0, |(_, top)| top * 0.85);
    scores
        .into_iter()
        .take(MAX_RESULTS)
        .take_while(|(_, score)| *score >= cutoff)
        .map(|(item, _)| item)
        .collect()
}

#[cfg(test)]
mod tests;
