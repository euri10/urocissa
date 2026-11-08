//! Versioned built-in definitions. Loading is pure; the caller caches prompt vectors
//! by (model ID, vocabulary version) and supplies them here, without HTTP or storage.
use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{AutoTagEngine, Concept, ConceptClassifier, ConceptMode, ScoreModel, Vocabulary};

// Explicit review is required to extend this allow-list. It prevents an imported
// Open Images class from introducing inferred personal attributes. Neutral people,
// scenes, objects, animals, activities and events are the only built-in Concepts.
const REVIEWED_IDS: &str = "person crowd portrait selfie baby child animal bird stork duck swan \
    cat dog horse cow sheep deer butterfly bee tree flower forest mountain beach sea lake river \
    waterfall snow sunset rainbow cloud garden park building house castle bridge street bicycle \
    car bus train boat airplane food cake bread pizza fruit coffee beer wine tent campfire hiking \
    swimming skiing cycling concert festival birthday wedding fireworks protest banner police flag \
    book computer guitar sculpture";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinConcept {
    pub id: String,
    pub label_en: String,
    pub label_fr: String,
    #[serde(default)]
    pub synonyms: Vec<String>,
    pub parents: Vec<String>,
    /// None means not calibrated: hierarchy/alias only, never a zero-shot guess.
    pub threshold: Option<f32>,
    pub open_images: String,
}

impl BuiltinConcept {
    fn with_vector(&self, vector: Vec<f32>) -> Concept {
        Concept {
            id: self.id.clone(),
            label_en: self.label_en.clone(),
            label_fr: self.label_fr.clone(),
            synonyms: self.synonyms.clone(),
            parents: self.parents.clone(),
            // Inactive Concepts still participate in aliases and hierarchy. The
            // catalog's classifier gate below prevents this placeholder firing.
            threshold: self.threshold.unwrap_or(0.5),
            vector,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinCatalog {
    schema_version: u32,
    pub version: String,
    pub model_id: String,
    prompt_template: String,
    pub concepts: Vec<BuiltinConcept>,
}

impl BuiltinCatalog {
    pub fn embedded() -> Result<Self> {
        Self::from_json(include_str!("../../../assets/concepts.json"))
    }

    pub fn from_json(source: &str) -> Result<Self> {
        let catalog: Self = serde_json::from_str(source).context("invalid built-in vocabulary")?;
        ensure!(catalog.schema_version == 1, "unsupported vocabulary schema");
        ensure!(
            !catalog.version.trim().is_empty(),
            "missing vocabulary version"
        );
        ensure!(
            !catalog.model_id.trim().is_empty(),
            "missing vocabulary model"
        );
        ensure!(
            catalog.prompt_template == "a photo of {label}",
            "unsupported scoring prompt"
        );
        ensure!(!catalog.concepts.is_empty(), "empty built-in vocabulary");
        for concept in &catalog.concepts {
            let label = if concept.id == "birthday" {
                "birthday party"
            } else {
                &concept.id
            };
            ensure!(
                REVIEWED_IDS.split_whitespace().any(|id| id == concept.id)
                    && concept.label_en == label,
                "Concept `{}` is outside the reviewed exclusion policy",
                concept.id
            );
            ensure!(
                !concept.open_images.trim().is_empty(),
                "missing calibration mapping"
            );
        }
        // Reuse all engine checks (names, aliases, thresholds, parents and cycles).
        Vocabulary::new(
            1,
            catalog
                .concepts
                .iter()
                .map(|c| c.with_vector(vec![1.0]))
                .collect(),
        )?;
        Ok(catalog)
    }

    /// Prompts in definition order. The text tower embeds these once per cache key.
    pub fn prompts(&self) -> Vec<String> {
        self.concepts
            .iter()
            .map(|c| self.prompt_template.replace("{label}", &c.label_en))
            .collect()
    }

    /// Bind exact-model vectors and the calibrated activation gate together. Callers
    /// must not bypass the gate by constructing a zero-shot engine from definitions.
    pub fn into_engine(
        self,
        model_id: &str,
        dim: usize,
        mut vectors: HashMap<String, Vec<f32>>,
        score_model: ScoreModel,
    ) -> Result<AutoTagEngine> {
        ensure!(
            model_id == self.model_id,
            "vocabulary/model mismatch; recalibrate first"
        );
        ensure!(
            vectors.len() == self.concepts.len(),
            "wrong Concept vector count"
        );
        let concepts = self
            .concepts
            .iter()
            .map(|c| {
                let vector = vectors
                    .remove(&c.id)
                    .with_context(|| format!("missing vector for `{}`", c.id))?;
                Ok(c.with_vector(vector))
            })
            .collect::<Result<Vec<_>>>()?;
        let active = self
            .concepts
            .iter()
            .filter(|c| c.threshold.is_some())
            .map(|c| c.id.clone())
            .collect();
        Ok(
            AutoTagEngine::new(Vocabulary::new(dim, concepts)?, score_model)
                .with_classifier(CalibratedOnly(active)),
        )
    }
}

struct CalibratedOnly(HashSet<String>);

impl ConceptClassifier for CalibratedOnly {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        if self.0.contains(&concept.id) {
            ConceptMode::ZeroShot
        } else {
            ConceptMode::Inactive
        }
    }
}

#[cfg(test)]
mod tests;
