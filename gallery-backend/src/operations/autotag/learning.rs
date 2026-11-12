//! Owner feedback only: unlabelled photos are never negative training examples.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{
    Classification, Concept, ConceptClassifier, ConceptMode, PhotoInput, ScoreModel, Vocabulary,
    check_vector, normalize_name,
};

const MIN_EXAMPLES: usize = 5;
/// Share of the owner's examples allowed to sit farther from the rest than the match cutoff.
const EXAMPLE_QUANTILE: f32 = 0.1;
/// Confidence slope per unit of cosine margin; only orders matches, 0.5 is the cutoff.
const MARGIN_SLOPE: f32 = 50.0;

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

/// Matching by the owner's own examples, for personal and uncalibrated built-in Concepts.
///
/// A photo matches when its nearest positive example is (a) at least as close as the
/// [`EXAMPLE_QUANTILE`] of the examples' leave-one-out nearest-example similarity, (b) at
/// least as close as any rejected photo is to the examples, and (c) closer to a positive
/// than to any rejected photo. Measured on a 6,372-photo `SigLIP2` library, a fixed cosine
/// cutoff to the examples' mean matched 461–1,659 photos for one Concept and 28 for another,
/// while this rule kept 20–125 and re-suggested almost no held-out Rejections
/// (`infra-urocissa-autotag-sjfz.22`). Every Rejection tightens it immediately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Examples {
    positives: Vec<Vec<f32>>,
    rejections: Vec<Vec<f32>>,
    cutoff: f32,
}

impl Examples {
    fn fit(positives: &[&[f32]], rejections: &[&[f32]]) -> Option<Self> {
        if positives.len() < MIN_EXAMPLES {
            return None;
        }
        let mut spread = (0..positives.len())
            .map(|i| nearest(positives, positives[i], Some(i)))
            .collect::<Vec<_>>();
        spread.sort_by(f32::total_cmp);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_precision_loss,
            clippy::cast_sign_loss,
            reason = "bounded photo counts"
        )]
        let quantile = spread[((spread.len() - 1) as f32 * EXAMPLE_QUANTILE) as usize];
        let cutoff = rejections
            .iter()
            .map(|rejection| nearest(positives, rejection, None))
            .fold(quantile, f32::max);
        cutoff.is_finite().then(|| Self {
            positives: positives.iter().map(|v| v.to_vec()).collect(),
            rejections: rejections.iter().map(|v| v.to_vec()).collect(),
            cutoff,
        })
    }

    fn probability(&self, embedding: &[f32]) -> f32 {
        let positive = nearest(&self.positives, embedding, None);
        let rejection = nearest(&self.rejections, embedding, None);
        sigmoid(MARGIN_SLOPE * (positive - self.cutoff).min(positive - rejection))
    }
}

/// Highest cosine to any vector in `set` except `skip`; `-inf` for an empty set.
fn nearest<V: AsRef<[f32]>>(set: &[V], embedding: &[f32], skip: Option<usize>) -> f32 {
    set.iter()
        .enumerate()
        .filter(|&(index, _)| Some(index) != skip)
        .map(|(_, vector)| dot(vector.as_ref(), embedding))
        .fold(f32::NEG_INFINITY, f32::max)
}

/// Derived coefficients/examples. Persist only alongside their exact model/vocabulary.
/// On restart we retrain from authored decisions and current embeddings, never stale weights.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Learning {
    active: BTreeSet<String>,
    examples: BTreeMap<String, Examples>,
    pub classifiers: BTreeMap<String, Logistic>,
}

impl Learning {
    /// Actual fitted state, rather than a prediction based only on example counts.
    pub fn method(&self, concept: &str) -> &'static str {
        if self.classifiers.contains_key(concept) {
            "learned"
        } else if self.examples.contains_key(concept) {
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
                // Display/search vector only; matching uses the examples themselves.
                for value in &mut prototype {
                    *value /= norm;
                }
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
            let examples_where = |positive: bool| {
                labelled
                    .iter()
                    .filter(|example| example.1 == positive)
                    .map(|example| example.0)
                    .collect::<Vec<&[f32]>>()
            };
            let (positives, rejections) = (examples_where(true), examples_where(false));
            // Personal and uncalibrated built-in Concepts have no usable prompt: their examples
            // decide, and Rejections tighten the match directly (an uncalibrated built-in never
            // fires zero-shot, so it could otherwise never be shown and rejected).
            if concept.id.starts_with("personal:") || !learning.active.contains(&concept.id) {
                if let Some(examples) = Examples::fit(&positives, &rejections) {
                    learning.examples.insert(concept.id.clone(), examples);
                }
            } else if positives.len() >= MIN_EXAMPLES && rejections.len() >= MIN_EXAMPLES {
                let examples = labelled
                    .iter()
                    .map(|&(embedding, positive)| {
                        (
                            embedding,
                            score.probability(dot(&concept.vector, embedding)),
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
}

impl ConceptClassifier for Learning {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        if self.classifiers.contains_key(&concept.id) || self.examples.contains_key(&concept.id) {
            ConceptMode::Classifier
        } else if self.active.contains(&concept.id) {
            ConceptMode::ZeroShot
        } else {
            ConceptMode::Inactive
        }
    }

    fn classify(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        if let Some(examples) = self.examples.get(&concept.id) {
            return Classification {
                probability: examples.probability(embedding),
                threshold: 0.5,
            };
        }
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

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

#[cfg(test)]
mod tests;
