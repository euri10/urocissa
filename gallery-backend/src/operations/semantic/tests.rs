use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;
use anyhow::{Result, ensure};

pub(crate) struct FakeEmbedder {
    pub online: AtomicBool,
    pub calls: AtomicUsize,
    pub model: Mutex<ModelInfo>,
}

impl Default for FakeEmbedder {
    fn default() -> Self {
        Self {
            online: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
            model: Mutex::new(ModelInfo {
                model_id: "semantic-fixture".into(),
                embedding_dim: 2,
                device: "CPU".into(),
                logit_scale: None,
                logit_bias: None,
            }),
        }
    }
}

impl Embedder for FakeEmbedder {
    fn info(&self) -> Result<ModelInfo> {
        ensure!(self.online.load(Ordering::Relaxed), "offline");
        Ok(self.model.lock().unwrap().clone())
    }
    fn embed_image(&self, _: Vec<u8>) -> Result<ImageEmbedding> {
        anyhow::bail!("not used")
    }
    fn embed_text(&self, _: &str) -> Result<ImageEmbedding> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ImageEmbedding {
            model_id: self.model.lock().unwrap().model_id.clone(),
            embedding: vec![1.0, 0.0],
        })
    }
}

#[test]
fn semantic_expression_composes_and_rejects_ambiguous_or_invalid_queries() {
    let filter = Expression::And(vec![
        Expression::Tag(crate::public::structure::expression::FilterValue::Value(
            "family".into(),
        )),
        Expression::Audience(crate::public::structure::audience::Audience::Private),
    ]);
    let query = Expression::And(vec![
        filter.clone(),
        Expression::Semantic("  enfant à vélo  ".into()),
    ]);
    let (text, remainder) = split_expression(&query).unwrap();
    assert_eq!(text, SearchQuery::Text("enfant à vélo".into()));
    assert_eq!(
        remainder,
        Expression::And(vec![filter, Expression::And(vec![])])
    );
    assert!(!remainder.contains_semantic());
    for invalid in [
        Expression::Semantic(" ".into()),
        Expression::Semantic("é".repeat(2001)),
        Expression::Not(Box::new(Expression::Semantic("bird".into()))),
        Expression::Or(vec![
            Expression::Semantic("bird".into()),
            Expression::Trashed(false),
        ]),
        Expression::And(vec![
            Expression::Semantic("bird".into()),
            Expression::Semantic("vélo".into()),
        ]),
        Expression::Trashed(false),
    ] {
        assert!(split_expression(&invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn similar_expression_validates_source_and_reuses_ranked_filter_rules() {
    let id = "a".repeat(64);
    let expression = Expression::Similar(id.clone());
    assert!(expression.contains_semantic());
    assert_eq!(
        split_expression(&expression).unwrap(),
        (SearchQuery::Similar(id.clone()), Expression::And(vec![]))
    );
    for invalid in [
        Expression::Similar(String::new()),
        Expression::Similar("x".repeat(64)),
        Expression::Similar("a".repeat(63)),
        Expression::Not(Box::new(expression.clone())),
        Expression::Or(vec![expression.clone(), Expression::Trashed(false)]),
        Expression::And(vec![
            expression.clone(),
            Expression::Semantic("bird".into()),
        ]),
        Expression::And(vec![expression.clone(), expression]),
    ] {
        assert!(split_expression(&invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn ranking_is_bounded_relative_to_the_best_and_deterministic() {
    assert_eq!(
        rank(vec![
            ("best", 0.8),
            ("near", 0.7),
            ("far", 0.5),
            ("nan", f32::NAN),
            ("negative", -0.2)
        ]),
        vec!["best", "near"]
    );
    let result = rank((0..1000).rev().map(|id| (id, 0.5)).collect());
    assert_eq!(result, (0..200).collect::<Vec<_>>());
    assert!(rank(vec![(0, -0.1), (1, 0.0)]).is_empty());
}

#[test]
fn query_cache_tracks_model_ttl_and_outages_and_selection_is_bound_to_its_filter() {
    let root = tempfile::tempdir().unwrap();
    let embedder = Arc::new(FakeEmbedder::default());
    let search = SemanticSearch::new(
        embedder.clone(),
        EmbeddingStore::open(&root.path().join("cache.redb")).unwrap(),
    );
    for text in ["enfant à vélo", "child on a bike"] {
        search.query_vector(text).unwrap();
        search.query_vector(text).unwrap();
    }
    assert_eq!(embedder.calls.load(Ordering::Relaxed), 2);
    embedder.model.lock().unwrap().model_id = "new-model".into();
    search.query_vector("enfant à vélo").unwrap();
    assert_eq!(embedder.calls.load(Ordering::Relaxed), 3);
    search.queries.lock().unwrap().back_mut().unwrap().created -= QUERY_TTL;
    search.query_vector("enfant à vélo").unwrap();
    assert_eq!(embedder.calls.load(Ordering::Relaxed), 4);
    embedder.online.store(false, Ordering::Relaxed);
    assert_eq!(
        search.query_vector("enfant à vélo").unwrap_err().kind,
        ErrorKind::Unavailable
    );

    let expression = Expression::Semantic("enfant à vélo".into());
    let selection = search.remember(expression.clone(), vec![], 2, 3);
    assert!(
        search
            .resume(&selection.summary.selection, &expression)
            .is_ok()
    );
    assert!(
        search
            .resume(
                &selection.summary.selection,
                &Expression::Semantic("different".into())
            )
            .is_err()
    );
    search
        .selections
        .lock()
        .unwrap()
        .back_mut()
        .unwrap()
        .accessed -= SELECTION_TTL;
    assert!(
        search
            .resume(&selection.summary.selection, &expression)
            .is_err()
    );
    for _ in 0..100 {
        search.remember(expression.clone(), vec![], 2, 3);
    }
    assert_eq!(search.selections.lock().unwrap().len(), MAX_SELECTIONS);
}

#[test]
fn cosine_scan_and_rank_ten_thousand_photos() {
    let dim = 768;
    let vector = vec![1.0 / 768.0_f32.sqrt(); dim];
    let photos = vec![vector.clone(); 10_000];
    let start = Instant::now();
    let results = rank(
        photos
            .iter()
            .enumerate()
            .map(|(id, photo)| {
                let score = photo.iter().zip(&vector).map(|(a, b)| a * b).sum();
                (id, score)
            })
            .collect(),
    );
    let elapsed = start.elapsed();
    eprintln!("10k × 768 cosine scan + rank: {elapsed:?}");
    assert_eq!(results.len(), MAX_RESULTS);
    assert!(
        elapsed < Duration::from_millis(if cfg!(debug_assertions) { 1000 } else { 200 }),
        "ranking budget exceeded: {elapsed:?}"
    );
}
