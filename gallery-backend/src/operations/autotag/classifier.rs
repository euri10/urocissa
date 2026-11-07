//! The per-Concept classifier hook. Learned classifiers and the personal-Concept gate plug in
//! here; engine callers never change.
use super::vocabulary::Concept;

/// How the engine scores a Concept. Consulted once per Concept when the engine is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConceptMode {
    /// The Concept never fires from scoring (for example a personal Concept below its photo
    /// gate). Hierarchy and aliases still apply: it is implied by a present descendant, and a
    /// User tag naming it still aliases it.
    Inactive,
    /// Fires when the zero-shot probability reaches the Concept's threshold.
    ZeroShot,
    /// [`ConceptClassifier::classify`] decides per photo.
    Classifier,
}

/// A per-photo decision: the Concept fires when `probability >= threshold`, and `probability`
/// becomes the Auto tag's confidence. A NaN in either field never fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Classification {
    pub probability: f32,
    pub threshold: f32,
}

/// Replaces the zero-shot probability and/or threshold per Concept.
///
/// Rejections, aliases, hierarchy and the display cap are applied after this hook, so no
/// classifier can override a Rejection.
pub trait ConceptClassifier: Send + Sync {
    fn mode(&self, _concept: &Concept) -> ConceptMode {
        ConceptMode::ZeroShot
    }

    /// Called only for Concepts in [`ConceptMode::Classifier`], once per scored photo that has
    /// neither rejected nor aliased the Concept. `embedding` is the validated photo embedding.
    fn classify(&self, concept: &Concept, _embedding: &[f32], zero_shot: f32) -> Classification {
        Classification {
            probability: zero_shot,
            threshold: concept.threshold,
        }
    }
}

/// Pure zero-shot scoring for every Concept; the engine default.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZeroShot;

impl ConceptClassifier for ZeroShot {}
