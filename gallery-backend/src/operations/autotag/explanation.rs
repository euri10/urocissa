//! Explain one existing decision without fitting, inference or state changes.
use serde::Serialize;

use super::{AutoTagEngine, AutoTagError, ConceptMode, PhotoInput, Placement, check_vector};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConceptExplanation {
    pub concept: String,
    pub decision: &'static str,
    pub zero_shot_score: f32,
    pub match_score: Option<f32>,
    pub threshold: Option<f32>,
    pub via_user_tags: Vec<String>,
    pub via_auto_tags: Vec<String>,
}

impl AutoTagEngine {
    pub fn explain(
        &self,
        photo: &PhotoInput<'_>,
        requested: &str,
    ) -> Result<Option<ConceptExplanation>, AutoTagError> {
        check_vector(photo.embedding, self.vocabulary().dim())
            .map_err(AutoTagError::InvalidEmbedding)?;
        let Some(concept) = self.vocabulary().concept(requested) else {
            return Ok(None);
        };
        let (zero_shot_score, classification) = self.explain_score(concept, photo.embedding);
        let tags = self.tag_photo(photo)?;
        let authored = photo
            .user_tags
            .iter()
            .filter_map(|tag| self.vocabulary().concept_named(tag))
            .collect::<Vec<_>>();
        let ancestor_of = |child: &str| {
            self.vocabulary()
                .ancestors(child)
                .any(|parent| parent.id == requested)
        };
        let mut via_user_tags = authored
            .iter()
            .filter(|child| {
                !photo.rejections.contains(&child.id.as_str()) && ancestor_of(&child.id)
            })
            .map(|child| child.id.clone())
            .collect::<Vec<_>>();
        via_user_tags.sort();
        via_user_tags.dedup();
        let via_auto_tags = tags
            .iter()
            .filter(|tag| tag.placement != Placement::Implied && ancestor_of(tag.concept))
            .map(|tag| tag.concept.to_owned())
            .collect();
        let decision = if photo.auto_tags_off {
            "off"
        } else if photo.rejections.contains(&requested) {
            "rejected"
        } else if authored.iter().any(|tag| tag.id == requested) {
            "authored"
        } else if let Some(tag) = tags.iter().find(|tag| tag.concept == requested) {
            match tag.placement {
                Placement::Displayed => "displayed",
                Placement::OverCap => "overCap",
                Placement::SuppressedAncestor => "suppressedAncestor",
                Placement::Implied => "implied",
            }
        } else if self.explain_mode(concept) == ConceptMode::Inactive {
            "inactive"
        } else {
            "belowThreshold"
        };
        Ok(Some(ConceptExplanation {
            concept: requested.to_owned(),
            decision,
            zero_shot_score,
            match_score: classification.map(|decision| decision.probability),
            threshold: classification.map(|decision| decision.threshold),
            via_user_tags,
            via_auto_tags,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::autotag::{
        Classification, Concept, ConceptClassifier, ScoreModel, Vocabulary,
    };

    struct Calibrated;
    impl ConceptClassifier for Calibrated {
        fn mode(&self, concept: &Concept) -> ConceptMode {
            if concept.id == "building" {
                ConceptMode::Inactive
            } else if concept.id == "bridge" {
                ConceptMode::Classifier
            } else {
                ConceptMode::ZeroShot
            }
        }

        fn classify(
            &self,
            _concept: &Concept,
            _embedding: &[f32],
            _zero_shot: f32,
        ) -> Classification {
            Classification {
                probability: 0.6,
                threshold: 0.8,
            }
        }
    }

    #[test]
    fn explains_rejection_authored_inheritance_and_below_threshold_without_mutation() {
        let concept = |id: &str, parents: &[&str], vector| Concept {
            id: id.into(),
            label: id.into(),
            synonyms: vec![],
            parents: parents.iter().map(|s| (*s).into()).collect(),
            threshold: 0.8,
            vector,
        };
        let engine = AutoTagEngine::new(
            Vocabulary::new(
                2,
                vec![
                    concept("building", &[], vec![1.0, 0.0]),
                    concept("house", &["building"], vec![1.0, 0.0]),
                    concept("bridge", &[], vec![0.0, 1.0]),
                ],
            )
            .unwrap(),
            ScoreModel::new(10.0, -5.0).unwrap(),
        )
        .with_classifier(Calibrated);
        let photo = PhotoInput {
            embedding: &[1.0, 0.0],
            user_tags: &[],
            rejections: &[],
            auto_tags_off: false,
        };
        let before = engine.tag_photo(&photo).unwrap();
        let inherited = engine.explain(&photo, "building").unwrap().unwrap();
        assert_eq!(inherited.decision, "implied");
        assert_eq!(inherited.via_auto_tags, ["house"]);
        assert!(inherited.threshold.is_none());
        let bridge = engine.explain(&photo, "bridge").unwrap().unwrap();
        assert_eq!(bridge.decision, "belowThreshold");
        assert!(bridge.match_score.unwrap() < bridge.threshold.unwrap());
        assert_eq!(bridge.match_score, Some(0.6));
        assert_ne!(bridge.match_score, Some(bridge.zero_shot_score));
        let authored = PhotoInput {
            user_tags: &["house"],
            ..photo
        };
        assert_eq!(
            engine
                .explain(&authored, "house")
                .unwrap()
                .unwrap()
                .decision,
            "authored"
        );
        let inherited = engine.explain(&authored, "building").unwrap().unwrap();
        assert_eq!(inherited.via_user_tags, ["house"]);
        assert!(inherited.via_auto_tags.is_empty());
        let rejected = PhotoInput {
            rejections: &["building", "house"],
            ..authored
        };
        assert_eq!(
            engine
                .explain(&rejected, "house")
                .unwrap()
                .unwrap()
                .decision,
            "rejected"
        );
        assert_eq!(
            engine
                .explain(&rejected, "building")
                .unwrap()
                .unwrap()
                .decision,
            "rejected"
        );
        let off = PhotoInput {
            auto_tags_off: true,
            ..rejected
        };
        assert_eq!(
            engine.explain(&off, "building").unwrap().unwrap().decision,
            "off"
        );
        assert!(engine.explain(&photo, "missing").unwrap().is_none());
        assert!(
            engine
                .explain(
                    &PhotoInput {
                        embedding: &[f32::NAN, 0.0],
                        ..photo
                    },
                    "bridge"
                )
                .is_err()
        );
        assert_eq!(before, engine.tag_photo(&photo).unwrap());
    }
}
