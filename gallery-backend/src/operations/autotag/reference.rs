//! Frozen public-data defaults, never a replacement for the owner's fitted matcher.
use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{
    BuiltinCatalog, BuiltinConcept, Classification, Concept, ConceptClassifier, ConceptMode,
    ScoreModel, check_vector,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceHead {
    pub weights: Vec<f32>,
    pub bias: f32,
    pub threshold: f32,
}

/// Public text-prompt vectors for the exact model; never replace owner feedback.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InsectGuard {
    schema_version: u32,
    model_id: String,
    spider_prompt: String,
    centipede_prompt: String,
    crab_prompt: String,
    spider: Vec<f32>,
    centipede: Vec<f32>,
    crab: Vec<f32>,
}

impl InsectGuard {
    fn embedded(model_id: &str) -> Result<Self> {
        let guard: Self = serde_json::from_str(include_str!("../../../assets/insect-guard.json"))?;
        ensure!(
            guard.schema_version == 1
                && guard.model_id == model_id
                && guard.spider_prompt == "a photo of spider"
                && guard.centipede_prompt == "a photo of centipede"
                && guard.crab_prompt == "a photo of crab"
                && check_vector(&guard.spider, 768).is_ok()
                && check_vector(&guard.centipede, 768).is_ok()
                && check_vector(&guard.crab, 768).is_ok(),
            "invalid insect guard"
        );
        Ok(guard)
    }

    pub(super) fn dim(&self) -> usize {
        self.spider.len()
    }

    fn accepts(&self, score: ScoreModel, embedding: &[f32], insect: f32) -> bool {
        let probability = |vector: &[f32]| {
            score.probability(embedding.iter().zip(vector).map(|(a, b)| a * b).sum())
        };
        insect > probability(&self.spider)
            && insect > probability(&self.centipede)
            && insect > probability(&self.crab)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferenceDefaults {
    schema_version: u32,
    baseline_version: String,
    vocabulary_version: String,
    model_id: String,
    embedding_dim: usize,
    concepts: Vec<BuiltinConcept>,
    parent_refinements: BTreeMap<String, Vec<String>>,
    heads: BTreeMap<String, ReferenceHead>,
}

impl ReferenceDefaults {
    pub(super) fn apply(self, catalog: &mut BuiltinCatalog) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.embedding_dim == 768,
            "unsupported public defaults"
        );
        ensure!(
            self.baseline_version == catalog.version && self.model_id == catalog.model_id,
            "public defaults baseline/model mismatch"
        );
        ensure!(
            !self.vocabulary_version.trim().is_empty()
                && self.vocabulary_version != catalog.version,
            "public defaults need a new vocabulary version"
        );
        catalog.concepts.extend(self.concepts);
        for (id, parents) in self.parent_refinements {
            let concept = catalog
                .concepts
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow::anyhow!("unknown refined Concept `{id}`"))?;
            concept.parents = parents;
        }
        for (id, head) in &self.heads {
            ensure!(
                catalog
                    .concepts
                    .iter()
                    .any(|c| c.id == *id && c.threshold.is_none()),
                "public reference must not replace active/unknown Concept `{id}`"
            );
            ensure!(
                head.weights.len() == self.embedding_dim
                    && head.weights.iter().all(|w| w.is_finite())
                    && head.bias.is_finite()
                    && head.threshold.is_finite()
                    && head.threshold > 0.0
                    && head.threshold < 1.0,
                "invalid public reference `{id}`"
            );
        }
        // ponytail: quarantine public heads with observed unrelated-photo false
        // positives until representative negatives support their requalification.
        // Owner-fitted classifiers remain independent of these fallback heads.
        catalog.version = format!("{}-quarantine-4", self.vocabulary_version);
        catalog.references.clear();
        catalog.insect_guard = Some(InsectGuard::embedded(&catalog.model_id)?);
        Ok(())
    }
}

pub struct ReferenceFallback<C> {
    base: C,
    heads: BTreeMap<String, ReferenceHead>,
    insect_guard: Option<(InsectGuard, ScoreModel)>,
}

impl<C> ReferenceFallback<C> {
    pub(super) fn new(base: C, heads: BTreeMap<String, ReferenceHead>) -> Self {
        Self {
            base,
            heads,
            insect_guard: None,
        }
    }

    pub(super) fn with_insect_guard(
        mut self,
        guard: Option<InsectGuard>,
        score: ScoreModel,
    ) -> Self {
        self.insect_guard = guard.map(|guard| (guard, score));
        self
    }
}

impl<C: ConceptClassifier> ConceptClassifier for ReferenceFallback<C> {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        match self.base.mode(concept) {
            ConceptMode::ZeroShot if concept.id == "insect" && self.insect_guard.is_some() => {
                ConceptMode::Classifier
            }
            // ponytail: only labels with representative precision evidence stay
            // zero-shot; owner-fitted classifiers retain their existing paths.
            ConceptMode::ZeroShot
                if !matches!(
                    concept.id.as_str(),
                    "animal" | "bird" | "cat" | "dog" | "food"
                ) =>
            {
                ConceptMode::Inactive
            }
            ConceptMode::Inactive if self.heads.contains_key(&concept.id) => {
                ConceptMode::Classifier
            }
            mode => mode,
        }
    }

    fn classify(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        if concept.id == "insect"
            && self.base.mode(concept) == ConceptMode::ZeroShot
            && let Some((guard, score)) = &self.insect_guard
        {
            return Classification {
                probability: if guard.accepts(*score, embedding, zero_shot) {
                    zero_shot
                } else {
                    0.0
                },
                threshold: concept.threshold,
            };
        }
        if self.base.mode(concept) == ConceptMode::Inactive
            && let Some(head) = self.heads.get(&concept.id)
        {
            let dot: f32 = head.weights.iter().zip(embedding).map(|(a, b)| a * b).sum();
            return Classification {
                probability: 1.0 / (1.0 + (-(dot + head.bias)).exp()),
                threshold: head.threshold,
            };
        }
        self.base.classify(concept, embedding, zero_shot)
    }

    fn classify_if_fired(
        &self,
        concept: &Concept,
        embedding: &[f32],
        zero_shot: f32,
    ) -> Option<Classification> {
        if concept.id == "insect"
            && self.base.mode(concept) == ConceptMode::ZeroShot
            && self.insect_guard.is_some()
        {
            if zero_shot < concept.threshold {
                return None;
            }
            let decision = self.classify(concept, embedding, zero_shot);
            return (decision.probability >= decision.threshold).then_some(decision);
        }
        if self.base.mode(concept) == ConceptMode::Inactive && self.heads.contains_key(&concept.id)
        {
            let decision = self.classify(concept, embedding, zero_shot);
            (decision.probability >= decision.threshold).then_some(decision)
        } else {
            self.base.classify_if_fired(concept, embedding, zero_shot)
        }
    }
}

#[cfg(test)]
mod public_quality;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::autotag::{AutoTagEngine, BuiltinCatalog, PhotoInput, ScoreModel};

    #[test]
    fn insect_guard_respects_owner_feedback_and_hierarchy() {
        struct Owner(ConceptMode);
        impl ConceptClassifier for Owner {
            fn mode(&self, concept: &Concept) -> ConceptMode {
                if concept.id == "insect" {
                    self.0
                } else {
                    ConceptMode::Inactive
                }
            }
            fn classify(&self, _: &Concept, _: &[f32], _: f32) -> Classification {
                Classification {
                    probability: 0.9,
                    threshold: 0.8,
                }
            }
        }
        let guard = InsectGuard {
            schema_version: 1,
            model_id: "fixture".into(),
            spider_prompt: "a photo of spider".into(),
            centipede_prompt: "a photo of centipede".into(),
            crab_prompt: "a photo of crab".into(),
            spider: vec![0.0, 1.0, 0.0, 0.0],
            centipede: vec![0.0, 0.0, 1.0, 0.0],
            crab: vec![0.0, 0.0, 0.0, 1.0],
        };
        let concept = |id: &str, parents: &[&str]| Concept {
            id: id.into(),
            label: id.into(),
            synonyms: vec![],
            parents: parents.iter().map(|p| (*p).into()).collect(),
            threshold: 0.002_214_877,
            vector: vec![1.0, 0.0, 0.0, 0.0],
        };
        let vocabulary = super::super::Vocabulary::new(
            4,
            vec![concept("animal", &[]), concept("insect", &["animal"])],
        )
        .unwrap();
        let score = ScoreModel::new(10.0, -5.0).unwrap();
        let engine = |mode| {
            AutoTagEngine::new(vocabulary.clone(), score).with_classifier(
                ReferenceFallback::new(Owner(mode), BTreeMap::new())
                    .with_insect_guard(Some(guard.clone()), score),
            )
        };
        let photo = |embedding, user_tags, rejections, auto_tags_off| PhotoInput {
            embedding,
            user_tags,
            rejections,
            auto_tags_off,
        };
        let fallback = engine(ConceptMode::ZeroShot);
        let result = fallback
            .tag_photo(&photo(&[1.0, 0.0, 0.0, 0.0], &[], &[], false))
            .unwrap();
        assert!(result.iter().any(|t| t.concept == "insect" && t.placement == super::super::Placement::Displayed));
        assert!(
            result
                .iter()
                .any(|t| t.concept == "animal" && t.placement == super::super::Placement::Implied)
        );
        for vector in [
            &[0.0, 1.0, 0.0, 0.0][..],
            &[0.0, 0.0, 1.0, 0.0][..],
            &[0.0, 0.0, 0.0, 1.0][..],
        ] {
            assert!(
                fallback
                    .tag_photo(&photo(vector, &[], &[], false))
                    .unwrap()
                    .is_empty()
            );
            assert!(
                engine(ConceptMode::Classifier)
                    .tag_photo(&photo(vector, &[], &[], false))
                    .unwrap()
                    .iter()
                    .any(|t| t.concept == "insect")
            );
        }
        assert!(
            !fallback
                .tag_photo(&photo(&[1.0, 0.0, 0.0, 0.0], &[], &["insect"], false))
                .unwrap()
                .iter()
                .any(|t| t.concept == "insect")
        );
        assert!(
            !fallback
                .tag_photo(&photo(&[1.0, 0.0, 0.0, 0.0], &["insect"], &[], false))
                .unwrap()
                .iter()
                .any(|t| t.concept == "insect")
        );
        assert!(
            fallback
                .tag_photo(&photo(&[1.0, 0.0, 0.0, 0.0], &[], &[], true))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn supplement_retains_every_original_threshold_and_quarantine() {
        let baseline =
            BuiltinCatalog::from_json(include_str!("../../../assets/concepts.json")).unwrap();
        let catalog = BuiltinCatalog::embedded().unwrap();
        assert_eq!(baseline.concepts.len(), 75);
        assert_eq!(catalog.concepts.len(), 82);
        assert!(catalog.references.is_empty());
        for original in baseline.concepts {
            let retained = catalog
                .concepts
                .iter()
                .find(|c| c.id == original.id)
                .unwrap();
            assert_eq!(retained.threshold, original.threshold, "{}", original.id);
            assert_eq!(retained.label_en, original.label_en);
            assert_eq!(retained.synonyms, original.synonyms);
            assert_eq!(retained.open_images, original.open_images);
            if !["bee", "butterfly"].contains(&original.id.as_str()) {
                assert_eq!(retained.parents, original.parents);
            }
        }
        assert!(
            catalog
                .concepts
                .iter()
                .find(|c| c.id == "flower")
                .unwrap()
                .threshold
                .is_none()
        );
        assert!(!catalog.references.contains_key("flower"));
        assert!(!catalog.references.contains_key("bridge"));
        assert!(!catalog.references.contains_key("building"));
    }

    #[test]
    fn malformed_public_defaults_are_refused_before_binding() {
        let source = include_str!("../../../assets/concept-defaults.json");
        let mutations: &[fn(&mut serde_json::Value)] = &[
            |v| v["model_id"] = "wrong".into(),
            |v| v["baseline_version"] = "wrong".into(),
            |v| v["vocabulary_version"] = "".into(),
            |v| v["embedding_dim"] = 2.into(),
            |v| v["heads"]["snow"]["weights"] = serde_json::json!([1.0]),
            |v| v["heads"]["snow"]["threshold"] = 0.into(),
            |v| {
                v["heads"]["cat"] = v["heads"]["snow"].clone();
            },
            |v| {
                v["heads"]["unknown"] = v["heads"]["snow"].clone();
            },
        ];
        for mutate in mutations {
            let mut value: serde_json::Value = serde_json::from_str(source).unwrap();
            mutate(&mut value);
            let defaults: ReferenceDefaults = serde_json::from_value(value).unwrap();
            let mut baseline =
                BuiltinCatalog::from_json(include_str!("../../../assets/concepts.json")).unwrap();
            assert!(defaults.apply(&mut baseline).is_err());
        }
        let mut catalog = BuiltinCatalog::embedded().unwrap();
        // Keep the historic dimension check even though production heads are quarantined.
        let defaults: ReferenceDefaults =
            serde_json::from_str(include_str!("../../../assets/concept-defaults.json")).unwrap();
        catalog.references = defaults.heads;
        let vectors = catalog
            .concepts
            .iter()
            .map(|c| (c.id.clone(), vec![1.0]))
            .collect();
        assert!(
            catalog
                .clone()
                .into_engine(
                    &catalog.model_id,
                    1,
                    vectors,
                    ScoreModel::new(1.0, 0.0).unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn public_fallback_yields_to_every_active_owner_mode() {
        struct Owner(ConceptMode);
        impl ConceptClassifier for Owner {
            fn mode(&self, _: &Concept) -> ConceptMode {
                self.0
            }
            fn classify(&self, _: &Concept, _: &[f32], _: f32) -> Classification {
                Classification {
                    probability: 0.1,
                    threshold: 0.8,
                }
            }
        }
        let concept = Concept {
            id: "animal".into(),
            label: "animal".into(),
            synonyms: vec![],
            parents: vec![],
            threshold: 0.9,
            vector: vec![1.0],
        };
        let head = ReferenceHead {
            weights: vec![10.0],
            bias: 0.0,
            threshold: 0.8,
        };
        for mode in [
            ConceptMode::Inactive,
            ConceptMode::ZeroShot,
            ConceptMode::Classifier,
        ] {
            let classifier =
                ReferenceFallback::new(Owner(mode), [("animal".into(), head.clone())].into());
            assert_eq!(
                classifier.mode(&concept),
                if mode == ConceptMode::Inactive {
                    ConceptMode::Classifier
                } else {
                    mode
                }
            );
            let decision = classifier.classify(&concept, &[1.0], 0.9);
            assert_eq!(decision.threshold.to_bits(), 0.8_f32.to_bits());
            assert_eq!(decision.probability > 0.8, mode == ConceptMode::Inactive);
            assert_eq!(
                classifier.classify_if_fired(&concept, &[1.0], 0.9),
                (decision.probability >= decision.threshold).then_some(decision)
            );
        }
    }

    #[test]
    fn quarantine_only_blocks_unsupported_zero_shot() {
        struct Owner(ConceptMode);
        impl ConceptClassifier for Owner {
            fn mode(&self, _: &Concept) -> ConceptMode {
                self.0
            }
            fn classify(&self, _: &Concept, _: &[f32], _: f32) -> Classification {
                Classification {
                    probability: 0.9,
                    threshold: 0.8,
                }
            }
        }
        for definition in BuiltinCatalog::embedded().unwrap().concepts {
            let concept = Concept {
                id: definition.id,
                label: definition.label_en,
                synonyms: definition.synonyms,
                parents: definition.parents,
                threshold: 0.5,
                vector: vec![1.0],
            };
            let supported = ["animal", "bird", "cat", "dog", "food"].contains(&concept.id.as_str());
            for (base, expected) in [
                (ConceptMode::Inactive, ConceptMode::Inactive),
                (
                    ConceptMode::ZeroShot,
                    if supported {
                        ConceptMode::ZeroShot
                    } else {
                        ConceptMode::Inactive
                    },
                ),
                (ConceptMode::Classifier, ConceptMode::Classifier),
            ] {
                let classifier = ReferenceFallback::new(Owner(base), BTreeMap::new());
                assert_eq!(classifier.mode(&concept), expected, "{}", concept.id);
                if base == ConceptMode::Classifier {
                    assert_eq!(
                        classifier.classify_if_fired(&concept, &[1.0], 0.1),
                        Some(Classification {
                            probability: 0.9,
                            threshold: 0.8,
                        })
                    );
                }
            }
        }
    }

    #[test]
    fn public_fallback_forwards_owner_fast_decisions() {
        struct FastOwner;
        impl ConceptClassifier for FastOwner {
            fn mode(&self, _: &Concept) -> ConceptMode {
                ConceptMode::Classifier
            }
            fn classify(&self, _: &Concept, _: &[f32], _: f32) -> Classification {
                panic!("materialization must forward the owner's fast decision")
            }
            fn classify_if_fired(&self, _: &Concept, _: &[f32], _: f32) -> Option<Classification> {
                Some(Classification {
                    probability: 0.75,
                    threshold: 0.5,
                })
            }
        }
        let concept = Concept {
            id: "fixture".into(),
            label: "fixture".into(),
            synonyms: vec![],
            parents: vec![],
            threshold: 0.5,
            vector: vec![1.0],
        };
        let classifier = ReferenceFallback::new(FastOwner, BTreeMap::new());
        assert_eq!(
            classifier.classify_if_fired(&concept, &[1.0], 0.0),
            Some(Classification {
                probability: 0.75,
                threshold: 0.5,
            })
        );
    }

    #[test]
    fn reference_defaults_respect_owner_veto_alias_and_off() {
        let mut catalog = BuiltinCatalog::embedded().unwrap();
        // Exercise historical fallback precedence without re-enabling it in production.
        let defaults: ReferenceDefaults =
            serde_json::from_str(include_str!("../../../assets/concept-defaults.json")).unwrap();
        catalog.references = defaults.heads;
        let vectors = catalog
            .concepts
            .iter()
            .map(|c| {
                let mut vector = vec![0.0; 768];
                vector[0] = 1.0;
                (c.id.clone(), vector)
            })
            .collect();
        let head = &catalog.references["snow"];
        let norm = head.weights.iter().map(|x| x * x).sum::<f32>().sqrt();
        let embedding = head.weights.iter().map(|x| x / norm).collect::<Vec<_>>();
        let engine = catalog
            .clone()
            .into_engine(
                &catalog.model_id,
                768,
                vectors,
                ScoreModel::new(1.0, -100.0).unwrap(),
            )
            .unwrap();
        let photo = |user_tags, rejections, auto_tags_off| PhotoInput {
            embedding: &embedding,
            user_tags,
            rejections,
            auto_tags_off,
        };
        let has_snow = |photo| {
            engine
                .tag_photo(&photo)
                .unwrap()
                .iter()
                .any(|t| t.concept == "snow")
        };
        assert!(has_snow(photo(&[], &[], false)));
        assert!(!has_snow(photo(&[], &["snow"], false)));
        let legacy = crate::operations::autotag::learning::personal_id("snow");
        let historical = [legacy.as_str()];
        assert!(!has_snow(photo(&[], &historical, false)));
        assert!(!has_snow(photo(&["snow"], &[], false)));
        assert!(engine.tag_photo(&photo(&[], &[], true)).unwrap().is_empty());
        // The same wrapper is used when learned state is refreshed.
        let inactive = super::super::learning::Learning::default();
        let engine = AutoTagEngine::new(
            engine.vocabulary().clone(),
            ScoreModel::new(1.0, -100.0).unwrap(),
        )
        .with_classifier(catalog.with_references(inactive, ScoreModel::new(1.0, -100.0).unwrap()));
        assert!(
            engine
                .tag_photo(&photo(&[], &[], false))
                .unwrap()
                .iter()
                .any(|t| t.concept == "snow")
        );
    }
}
