//! Background materialization. HTTP readers never run inference or modify V6 records.
use std::{collections::HashMap, sync::Mutex};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    autotag::{AliasIndex, AutoTagEngine, BuiltinCatalog, PhotoInput, Placement, ScoreModel},
    embedding::client::{Embedder, ModelInfo},
};
use crate::{
    public::{db::tree::TREE, structure::object::ObjectType},
    storage::{
        auto_tags::{ConceptVectors, MaterializedTags, StoredAutoTag},
        embeddings::EmbeddingStore,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Input {
    version: u32,
    tags: Vec<String>,
    enabled: bool,
    rejections: Vec<String>,
}
type PhotoInputs = HashMap<String, Input>;
type TagCounts = Vec<(String, usize)>;

struct Prepared {
    model: String,
    engine: AutoTagEngine,
    aliases: AliasIndex,
    seen: HashMap<String, Input>,
}

pub struct AutoTags {
    store: EmbeddingStore,
    catalog: BuiltinCatalog,
    prepared: Mutex<Option<Prepared>>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "Fields share the owner response Auto tags namespace"
)]
pub struct AutoTagStatus {
    pub auto_tags: Vec<StoredAutoTag>,
    pub auto_tags_enabled: bool,
    pub auto_tags_pending: bool,
}

impl AutoTags {
    #[cfg(test)]
    pub(crate) fn test_store(&self) -> EmbeddingStore {
        self.store.clone()
    }

    pub fn new(store: EmbeddingStore) -> Result<Self> {
        Ok(Self {
            store,
            catalog: BuiltinCatalog::embedded()?,
            prepared: Mutex::new(None),
        })
    }

    /// Cache key includes the immutable vocabulary release (thresholds and labels too).
    /// A failed text tower never prevents photo embeddings from being recorded.
    pub fn prepare(&self, model: &ModelInfo, embedder: &dyn Embedder) -> Result<()> {
        let mut prepared = self.prepared.lock().unwrap();
        if prepared.as_ref().is_some_and(|p| p.model == model.model_id) {
            return Ok(());
        }
        ensure!(
            model.model_id == self.catalog.model_id,
            "Auto tags require a calibrated model"
        );
        let vectors = if let Some(cached) = self
            .store
            .concept_vectors(&model.model_id, &self.catalog.version)?
        {
            cached
        } else {
            let scale = model.logit_scale.context("tagger omitted logit scale")?;
            let bias = model.logit_bias.context("tagger omitted logit bias")?;
            let prompts = self.catalog.prompts();
            let mut vectors = Vec::new();
            for chunk in prompts.chunks(64) {
                let batch = embedder.embed_texts(chunk)?;
                ensure!(batch.len() == chunk.len(), "wrong Concept vector count");
                for vector in &batch {
                    vector.validate(model)?;
                }
                vectors.extend(batch.into_iter().map(|v| v.embedding));
            }
            let cached = ConceptVectors {
                scale,
                bias,
                vectors: self
                    .catalog
                    .concepts
                    .iter()
                    .map(|c| c.id.clone())
                    .zip(vectors)
                    .collect(),
            };
            // Validate before making a durable cache entry.
            self.catalog.clone().into_engine(
                &model.model_id,
                model.embedding_dim,
                cached.vectors.clone(),
                ScoreModel::new(scale, bias)?,
            )?;
            self.store
                .put_concept_vectors(&model.model_id, &self.catalog.version, &cached)?;
            cached
        };
        let engine = self.catalog.clone().into_engine(
            &model.model_id,
            model.embedding_dim,
            vectors.vectors,
            ScoreModel::new(vectors.scale, vectors.bias)?,
        )?;
        *prepared = Some(Prepared {
            model: model.model_id.clone(),
            engine,
            aliases: AliasIndex::default(),
            seen: HashMap::new(),
        });
        Ok(())
    }

    pub fn reconcile(&self) -> Result<()> {
        // Serialize input snapshots and derived writes with authored decisions. Otherwise
        // an older worker snapshot could overwrite the result of a completed rejection.
        let _persistence = TREE
            .persistence_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("tree persistence lock poisoned"))?;
        let (inputs, counts) = gallery_inputs()?;
        self.materialize(&inputs, &counts)
    }

    /// Missing vectors are remembered too: backfill must not re-read every pending
    /// photo after each new embedding. The writer invalidates only its completed ID.
    pub fn photo_embedded(&self, id: &str) {
        if let Some(prepared) = self.prepared.lock().unwrap().as_mut() {
            prepared.seen.remove(id);
        }
    }

    fn materialize(&self, inputs: &PhotoInputs, counts: &[(String, usize)]) -> Result<()> {
        let mut guard = self.prepared.lock().unwrap();
        let Some(prepared) = guard.as_mut() else {
            return Ok(());
        };
        let aliases = AliasIndex::build(
            prepared.engine.vocabulary(),
            counts.iter().map(|(tag, count)| (tag.as_str(), *count)),
        );
        if prepared.aliases != aliases {
            prepared.engine.set_aliases(&aliases);
            prepared.aliases = aliases;
            prepared.seen.clear();
        }
        let mut rows = Vec::new();
        let mut completed = Vec::new();
        for (id, input) in inputs {
            if prepared.seen.get(id) == Some(input) {
                continue;
            }
            let Some(embedding) = self
                .store
                .get(id, &prepared.model, 0)?
                .filter(|e| e.thumbnail_version == input.version)
            else {
                completed.push((id.clone(), input.clone()));
                continue;
            };
            let user_tags: Vec<_> = input.tags.iter().map(String::as_str).collect();
            let rejections: Vec<_> = input.rejections.iter().map(String::as_str).collect();
            let tags = prepared
                .engine
                .tag_photo(&PhotoInput {
                    embedding: &embedding.vector,
                    user_tags: &user_tags,
                    rejections: &rejections,
                    auto_tags_off: !input.enabled,
                })?
                .into_iter()
                .map(|tag| StoredAutoTag {
                    concept: tag.concept.to_owned(),
                    label: tag.label.to_owned(),
                    confidence: tag.confidence,
                    displayed: tag.placement == Placement::Displayed,
                })
                .collect();
            rows.push((
                id.clone(),
                MaterializedTags {
                    enabled: input.enabled,
                    model_id: prepared.model.clone(),
                    vocabulary_version: self.catalog.version.clone(),
                    thumbnail_version: input.version,
                    tags,
                },
            ));
            completed.push((id.clone(), input.clone()));
        }
        self.store.put_auto_tags(&rows)?;
        prepared.seen.retain(|id, _| inputs.contains_key(id));
        prepared.seen.extend(completed);
        Ok(())
    }

    pub fn status(&self, id: &str, version: u32, enabled: bool) -> Result<AutoTagStatus> {
        if !enabled {
            return Ok(AutoTagStatus::default());
        }
        let model = self.store.model()?;
        let row = self.store.auto_tags(id)?.filter(|row| {
            row.enabled
                && row.thumbnail_version == version
                && row.vocabulary_version == self.catalog.version
                && model.as_ref().is_some_and(|m| m.model_id == row.model_id)
        });
        Ok(AutoTagStatus {
            auto_tags_pending: row.is_none(),
            auto_tags: row.map_or_else(Vec::new, |r| r.tags),
            auto_tags_enabled: true,
        })
    }
}

fn gallery_inputs() -> Result<(PhotoInputs, TagCounts)> {
    let settings = TREE.store.auto_tag_settings()?;
    let mut rejections = TREE.store.auto_tag_rejections()?;
    let state = TREE
        .state
        .read()
        .map_err(|_| anyhow::anyhow!("tree lock poisoned"))?;
    let mut inputs = HashMap::new();
    for slot in state.order.iter() {
        let Some(record) = state
            .get(*slot)
            .filter(|r| r.object_type == ObjectType::Image)
        else {
            continue;
        };
        inputs.insert(
            record.id.to_string(),
            Input {
                version: record.cache_version,
                tags: Vec::new(),
                enabled: settings.enabled(record.id.as_str())?,
                rejections: rejections.remove(record.id.as_str()).unwrap_or_default(),
            },
        );
    }
    let mut counts = Vec::new();
    for (tag, members) in state.query.tags.iter() {
        let mut count = 0;
        for ordinal in members.iter() {
            if let Some(record) = state.slot_for_ordinal(ordinal).and_then(|s| state.get(s))
                && let Some(input) = inputs.get_mut(record.id.as_str())
            {
                input.tags.push(tag.clone());
                count += 1;
            }
        }
        counts.push((tag.clone(), count));
    }
    for input in inputs.values_mut() {
        input.tags.sort();
    }
    Ok((inputs, counts))
}

/// Called only after the response's Owner gate, or behind `GuardAuth`.
pub fn owner_status(id: &str, version: u32) -> Result<Option<AutoTagStatus>> {
    let Some(service) = super::embedding::service() else {
        return Ok(None);
    };
    let enabled = TREE.store.auto_tag_settings()?.enabled(id)?;
    let mut status = service.auto_tags.get().map_or_else(
        || {
            Ok::<_, anyhow::Error>(Some(AutoTagStatus {
                auto_tags_enabled: enabled,
                auto_tags_pending: enabled,
                ..Default::default()
            }))
        },
        |tags| {
            Ok(Some(tags.status(id, version, enabled).unwrap_or_else(
                |error| {
                    log::warn!("Could not read derived Auto tags: {error:#}");
                    AutoTagStatus {
                        auto_tags_enabled: enabled,
                        auto_tags_pending: enabled,
                        ..Default::default()
                    }
                },
            )))
        },
    )?;
    // Authored decisions win even if derived storage is stale or the worker is offline.
    if let Some(status) = &mut status {
        let rejected = TREE.store.auto_tag_rejections_for(id)?;
        let tags = TREE.store.read(|reader| {
            Ok::<_, anyhow::Error>(
                reader
                    .get(id)?
                    .map(|r| r.into_value().tag().iter().cloned().collect())
                    .unwrap_or_default(),
            )
        })?;
        let aliases = super::tag_feedback::concepts_for_tags(&tags);
        status
            .auto_tags
            .retain(|tag| !rejected.contains(&tag.concept) && !aliases.contains(&tag.concept));
    }
    Ok(status)
}

/// Decisions use cached vectors only; ML availability never gates authored changes.
pub fn rematerialize() {
    if let Some(tags) = super::embedding::service().and_then(|s| s.auto_tags.get())
        && let Err(error) = tags.reconcile()
    {
        log::warn!("Could not rematerialize Auto tags: {error:#}");
    }
    super::embedding::wake();
}

#[cfg(test)]
mod tests;
