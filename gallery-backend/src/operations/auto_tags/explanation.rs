//! Owner-only, single-photo diagnostic. No model call, fitting or database writes.
use serde::Serialize;

use super::{AutoTags, Result, TREE};
use crate::operations::autotag::{ConceptExplanation, PhotoInput};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoExplanation {
    #[serde(flatten)]
    pub explanation: ConceptExplanation,
    pub learning_mode: String,
    pub learning_pending: bool,
}

impl AutoTags {
    pub fn explanation(
        &self,
        id: &str,
        version: u32,
        concept: &str,
    ) -> Result<Option<PhotoExplanation>> {
        let model = self.store.model()?;
        let Some(model) = model else { return Ok(None) };
        let Some(embedding) = self
            .store
            .get(id, &model.model_id, 0)?
            .filter(|embedding| embedding.thumbnail_version == version)
        else {
            return Ok(None);
        };
        let enabled = TREE.store.auto_tag_settings()?.enabled(id)?;
        let rejected = crate::operations::tag_feedback::rejections_for(id)?;
        let authored = TREE.store.read(|reader| {
            Ok::<_, anyhow::Error>(
                reader
                    .get(id)?
                    .map(|row| row.into_value().tag().iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default(),
            )
        })?;
        let user_tags = authored.iter().map(String::as_str).collect::<Vec<_>>();
        let rejections = rejected.iter().map(String::as_str).collect::<Vec<_>>();
        let pending = !self.recalculation_status().up_to_date;
        let guard = self.prepared.lock().unwrap();
        let Some(prepared) = guard.as_ref().filter(|p| p.model == model.model_id) else {
            return Ok(None);
        };
        let explanation = prepared.engine.explain(
            &PhotoInput {
                embedding: &embedding.vector,
                user_tags: &user_tags,
                rejections: &rejections,
                auto_tags_off: !enabled,
            },
            concept,
        )?;
        Ok(explanation.map(|explanation| PhotoExplanation {
            explanation,
            learning_mode: prepared
                .learning_methods
                .get(concept)
                .cloned()
                .unwrap_or_else(|| "pending".into()),
            learning_pending: prepared.learning_dirty || pending,
        }))
    }
}
