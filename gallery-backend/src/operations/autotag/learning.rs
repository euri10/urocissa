//! Owner feedback only: unlabelled photos are never negative training examples.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{
    Classification, Concept, ConceptClassifier, ConceptMode, PhotoInput, ScoreModel, Vocabulary,
    check_vector, normalize_name,
};

const MIN_EXAMPLES: usize = 5;
const PERSONAL_SIMILARITY: f32 = 0.8;

/// The ID depends on the normalized authored name, never the model or vocabulary release.
pub fn personal_id(tag: &str) -> String {
    format!("personal:{}", blake3::hash(normalize_name(tag).as_bytes()))
}

/// One deterministic spelling per personal Concept. Built-in aliases remain built-in.
pub fn personal_names<'a>(
    vocabulary: &Vocabulary,
    tags: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, String> {
    let mut names = BTreeMap::<String, String>::new();
    for tag in tags {
        if normalize_name(tag).is_empty() || vocabulary.concept_named(tag).is_some() {
            continue;
        }
        names
            .entry(personal_id(tag))
            .and_modify(|label| {
                if tag < label.as_str() {
                    tag.clone_into(label);
                }
            })
            .or_insert_with(|| tag.to_owned());
    }
    names
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logistic {
    weights: Vec<f32>,
    zero_shot_weight: f32,
    bias: f32,
}

impl Logistic {
    fn probability(&self, embedding: &[f32], zero_shot: f32) -> f32 {
        sigmoid(dot(&self.weights, embedding) + self.zero_shot_weight * zero_shot + self.bias)
    }

    fn fit(examples: &[(&[f32], f32, bool)], dim: usize) -> Self {
        let mut model = Self {
            weights: vec![0.0; dim],
            zero_shot_weight: 0.0,
            bias: 0.0,
        };
        let positives = examples.iter().filter(|e| e.2).count();
        let negatives = examples.len() - positives;
        let mut gradient = vec![0.0; dim];
        // Fixed initialization, ordering and iterations: rebuilding needs no random seed.
        // Class balancing keeps a large rejection batch from erasing rare positives.
        for _ in 0..160 {
            gradient.fill(0.0);
            let mut score_gradient = 0.0;
            let mut bias_gradient = 0.0;
            for &(embedding, zero_shot, positive) in examples {
                #[expect(clippy::cast_precision_loss, reason = "bounded photo counts")]
                let balance = 0.5 / if positive { positives } else { negatives } as f32;
                let error =
                    (model.probability(embedding, zero_shot) - f32::from(positive)) * balance;
                for (gradient, value) in gradient.iter_mut().zip(embedding) {
                    *gradient += error * value;
                }
                score_gradient += error * zero_shot;
                bias_gradient += error;
            }
            for (weight, gradient) in model.weights.iter_mut().zip(&gradient) {
                *weight -= 2.0 * (gradient + 0.002 * *weight);
            }
            model.zero_shot_weight -= 2.0 * (score_gradient + 0.002 * model.zero_shot_weight);
            model.bias -= 2.0 * bias_gradient;
        }
        model
    }
}

/// Derived coefficients/prototypes. Persist only alongside their exact model/vocabulary.
/// On restart we retrain from authored decisions and current embeddings, never stale weights.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Learning {
    active: BTreeSet<String>,
    prototypes: BTreeMap<String, Vec<f32>>,
    pub classifiers: BTreeMap<String, Logistic>,
}

impl Learning {
    /// Actual fitted state, rather than a prediction based only on example counts.
    pub fn method(&self, concept: &str) -> &'static str {
        if self.classifiers.contains_key(concept) {
            "learned"
        } else if self.prototypes.contains_key(concept) {
            "similarity"
        } else if self.active.contains(concept) {
            "builtin"
        } else {
            "inactive"
        }
    }

    /// `photos` may contain only cached, current-version embeddings. IDs canonicalize order;
    /// aliases count once per photo, and a Rejection wins over a contradictory User tag.
    pub fn fit(
        base: &Vocabulary,
        active: BTreeSet<String>,
        score: ScoreModel,
        photos: &[(&str, PhotoInput<'_>)],
    ) -> Result<(Vocabulary, Self)> {
        let mut photos = photos.to_vec();
        photos.sort_unstable_by_key(|(id, _)| *id);
        photos.dedup_by_key(|(id, _)| *id);
        photos.retain(|(_, photo)| check_vector(photo.embedding, base.dim()).is_ok());
        let names = personal_names(
            base,
            photos
                .iter()
                .flat_map(|(_, photo)| photo.user_tags.iter().copied()),
        );
        let mut concepts = base.concepts().to_vec();
        let mut learning = Self {
            active,
            ..Self::default()
        };
        for (id, label) in names {
            let positives = photos.iter().filter(|(_, photo)| {
                !photo.rejections.contains(&id.as_str())
                    && photo.user_tags.iter().any(|tag| personal_id(tag) == id)
            });
            let mut prototype = vec![0.0; base.dim()];
            let mut count = 0;
            for (_, photo) in positives {
                count += 1;
                for (sum, value) in prototype.iter_mut().zip(photo.embedding) {
                    *sum += value;
                }
            }
            let norm = dot(&prototype, &prototype).sqrt();
            if count >= MIN_EXAMPLES && norm > 1e-6 {
                for value in &mut prototype {
                    *value /= norm;
                }
                learning.prototypes.insert(id.clone(), prototype.clone());
            } else {
                // Inactive placeholder still permits stable aliases and Rejections.
                prototype.fill(0.0);
                prototype[0] = 1.0;
            }
            concepts.push(Concept {
                id,
                label,
                synonyms: vec![],
                parents: vec![],
                threshold: 0.5,
                vector: prototype,
            });
        }
        let vocabulary = Vocabulary::new(base.dim(), concepts)?;
        for concept in vocabulary.concepts() {
            // (embedding, owner says positive) for every photo the owner labelled.
            let labelled = photos
                .iter()
                .filter_map(|(_, photo)| {
                    let rejected = photo.rejections.contains(&concept.id.as_str());
                    let positive = photo.user_tags.iter().any(|tag| {
                        vocabulary
                            .concept_named(tag)
                            .is_some_and(|c| c.id == concept.id)
                    });
                    (positive || rejected).then_some((photo.embedding, positive && !rejected))
                })
                .collect::<Vec<_>>();
            let positives = labelled.iter().filter(|e| e.1).count();
            // An uncalibrated built-in never fires zero-shot, so it can never be shown and
            // rejected: owner positives alone must activate it, exactly like a personal tag.
            if !concept.id.starts_with("personal:")
                && !learning.active.contains(&concept.id)
                && positives >= MIN_EXAMPLES
                && let Some(prototype) =
                    normalized_mean(labelled.iter().filter(|e| e.1).map(|e| e.0), base.dim())
            {
                learning.prototypes.insert(concept.id.clone(), prototype);
            }
            if positives >= MIN_EXAMPLES && labelled.len() - positives >= MIN_EXAMPLES {
                let examples = labelled
                    .iter()
                    .map(|&(embedding, positive)| {
                        (
                            embedding,
                            learning.zero_shot(concept, embedding, score),
                            positive,
                        )
                    })
                    .collect::<Vec<_>>();
                learning
                    .classifiers
                    .insert(concept.id.clone(), Logistic::fit(&examples, base.dim()));
            }
        }
        Ok((vocabulary, learning))
    }

    /// The prior score a classifier refines. Training and prediction must use the same one:
    /// similarity to the owner's examples once a Concept matches by them, otherwise the
    /// built-in prompt's calibrated probability.
    fn zero_shot(&self, concept: &Concept, embedding: &[f32], score: ScoreModel) -> f32 {
        match self.similarity_prototype(concept) {
            Some(prototype) => similarity(prototype, embedding),
            None => score.probability(dot(&concept.vector, embedding)),
        }
    }

    /// Personal Concepts always match by example; calibrated built-ins never do.
    fn similarity_prototype<'a>(&'a self, concept: &'a Concept) -> Option<&'a [f32]> {
        if concept.id.starts_with("personal:") {
            Some(&concept.vector)
        } else if self.active.contains(&concept.id) {
            None
        } else {
            self.prototypes.get(&concept.id).map(Vec::as_slice)
        }
    }
}

impl ConceptClassifier for Learning {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        if self.classifiers.contains_key(&concept.id) || self.prototypes.contains_key(&concept.id) {
            ConceptMode::Classifier
        } else if self.active.contains(&concept.id) {
            ConceptMode::ZeroShot
        } else {
            ConceptMode::Inactive
        }
    }

    fn classify(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        let zero_shot = self
            .similarity_prototype(concept)
            .map_or(zero_shot, |prototype| similarity(prototype, embedding));
        self.classifiers.get(&concept.id).map_or(
            Classification {
                probability: zero_shot,
                threshold: 0.5,
            },
            |classifier| Classification {
                probability: classifier.probability(embedding, zero_shot),
                threshold: 0.8,
            },
        )
    }
}

/// Probability-like similarity to the owner's examples; 0.5 at cosine [`PERSONAL_SIMILARITY`].
fn similarity(prototype: &[f32], embedding: &[f32]) -> f32 {
    sigmoid(12.0 * (dot(prototype, embedding) - PERSONAL_SIMILARITY))
}

fn normalized_mean<'a>(vectors: impl Iterator<Item = &'a [f32]>, dim: usize) -> Option<Vec<f32>> {
    let mut mean = vec![0.0; dim];
    for vector in vectors {
        for (sum, value) in mean.iter_mut().zip(vector) {
            *sum += value;
        }
    }
    let norm = dot(&mean, &mean).sqrt();
    (norm > 1e-6).then(|| mean.into_iter().map(|value| value / norm).collect())
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

#[cfg(test)]
mod tests;
