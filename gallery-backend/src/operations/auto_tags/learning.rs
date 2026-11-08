//! Retrain only when authored examples (or their embeddings) change, not per backfill photo.
use super::{
    AliasIndex, AutoTagEngine, AutoTags, HashMap, PhotoInput, PhotoInputs, Prepared, Result,
};
use crate::operations::autotag::learning::Learning;

impl AutoTags {
    pub(super) fn refresh_learning(
        &self,
        prepared: &mut Prepared,
        inputs: &PhotoInputs,
    ) -> Result<()> {
        let feedback = inputs
            .iter()
            .filter(|(_, input)| !input.tags.is_empty() || !input.rejections.is_empty())
            .map(|(id, input)| {
                let mut input = input.clone();
                // Opting out affects predictions, never the owner's training examples.
                input.enabled = true;
                (id.clone(), input)
            })
            .collect::<HashMap<_, _>>();
        if !prepared.learning_dirty && prepared.feedback == feedback {
            return Ok(());
        }
        let mut examples = Vec::new();
        for (id, input) in &feedback {
            if let Some(embedding) = self
                .store
                .get(id, &prepared.model, 0)?
                .filter(|e| e.thumbnail_version == input.version)
            {
                examples.push((id, input, embedding));
            }
        }
        let names = examples
            .iter()
            .map(|(_, input, _)| {
                (
                    input.tags.iter().map(String::as_str).collect::<Vec<_>>(),
                    input
                        .rejections
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let photos = examples
            .iter()
            .zip(&names)
            .map(|((id, _, embedding), (tags, rejections))| {
                (
                    id.as_str(),
                    PhotoInput {
                        embedding: &embedding.vector,
                        user_tags: tags,
                        rejections,
                        auto_tags_off: false,
                    },
                )
            })
            .collect::<Vec<_>>();
        let active = self
            .catalog
            .concepts
            .iter()
            .filter(|concept| concept.threshold.is_some())
            .map(|concept| concept.id.clone())
            .collect();
        let (vocabulary, learning) =
            Learning::fit(&prepared.base, active, prepared.score, &photos)?;
        self.store
            .put_learning(&prepared.model, &self.catalog.version, &learning)?;
        prepared.engine = AutoTagEngine::new(vocabulary, prepared.score).with_classifier(learning);
        prepared.feedback = feedback;
        prepared.learning_dirty = false;
        prepared.aliases = AliasIndex::default();
        prepared.seen.clear();
        Ok(())
    }
}
