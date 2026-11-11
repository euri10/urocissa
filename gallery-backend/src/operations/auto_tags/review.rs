//! Read current predictions and authored examples without inference or new storage.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{AutoTags, Result, gallery_inputs};
use crate::operations::{autotag::check_vector, tag_feedback};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub concept: String,
    pub label: String,
    pub positive_examples: usize,
    pub negative_examples: usize,
    pub learning_mode: String,
}

pub struct ConceptReview {
    pub summary: ReviewSummary,
    pub photos: Vec<(String, u32, f32)>,
}

impl AutoTags {
    pub fn review(&self, concept: String, label: String) -> Result<ConceptReview> {
        let (inputs, _) = gallery_inputs()?;
        let model = self.store.model()?;
        let embeddings = model
            .as_ref()
            .map(|model| self.store.embeddings(&model.model_id))
            .transpose()?
            .unwrap_or_default();
        let mut confirmed = HashSet::new();
        let mut positive_examples = 0;
        let mut negative_examples = 0;
        for (id, input) in &inputs {
            let positive = input
                .tags
                .iter()
                .any(|tag| tag_feedback::concept_id(tag) == concept);
            if positive {
                confirmed.insert(id.as_str());
            }
            // These are the exact current-embedding gates used by learning, including
            // authored examples whose own Auto tags are switched off.
            if let Some(model) = &model
                && embeddings.get(id).is_some_and(|embedding| {
                    embedding.thumbnail_version == input.version
                        && check_vector(&embedding.vector, model.embedding_dim).is_ok()
                })
            {
                if input.rejections.contains(&concept) {
                    negative_examples += 1;
                } else if positive {
                    positive_examples += 1;
                }
            }
        }
        let mut photos = self
            .search_rows()?
            .into_iter()
            .filter_map(|(id, row)| {
                let input = inputs.get(&id)?;
                if !input.enabled
                    || input.version != row.thumbnail_version
                    || confirmed.contains(id.as_str())
                {
                    return None;
                }
                row.tags
                    .iter()
                    .find(|tag| tag.concept == concept)
                    .map(|tag| (id, row.thumbnail_version, tag.confidence))
            })
            .collect::<Vec<_>>();
        photos.sort_unstable_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        let learning_mode = self
            .prepared
            .lock()
            .unwrap()
            .as_ref()
            .filter(|prepared| {
                model
                    .as_ref()
                    .is_some_and(|model| model.model_id == prepared.model)
            })
            .and_then(|prepared| prepared.learning_methods.get(&concept))
            .cloned()
            .unwrap_or_else(|| "pending".into());
        Ok(ConceptReview {
            summary: ReviewSummary {
                concept,
                label,
                positive_examples,
                negative_examples,
                learning_mode,
            },
            photos,
        })
    }
}
