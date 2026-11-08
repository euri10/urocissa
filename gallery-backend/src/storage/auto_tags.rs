//! Authored preferences stay in the main database; all predictions are disposable.
use anyhow::Result;
use redb::{Durability, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use super::{DataStore, embeddings::EmbeddingStore};

const OFF: TableDefinition<&str, u8> = TableDefinition::new("auto_tags_off_v1");
pub(super) const REJECTIONS: TableDefinition<(&str, &str), u8> =
    TableDefinition::new("auto_tag_rejections_v1");
const CONCEPTS: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("concept_vectors_v1");
const TAGS: TableDefinition<&str, &[u8]> = TableDefinition::new("auto_tags_v1");
const LEARNING: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("concept_learning_v1");

pub struct AutoTagSettings(ReadOnlyTable<&'static str, u8>);

impl AutoTagSettings {
    pub fn enabled(&self, id: &str) -> Result<bool> {
        Ok(self.0.get(id)?.is_none())
    }
}

impl DataStore {
    pub(super) fn initialize_auto_tag_settings(&self) -> Result<()> {
        let write = self.db.begin_write()?;
        write.open_table(OFF)?;
        write.open_table(REJECTIONS)?;
        write.commit()?;
        Ok(())
    }

    pub fn auto_tag_settings(&self) -> Result<AutoTagSettings> {
        Ok(AutoTagSettings(self.db.begin_read()?.open_table(OFF)?))
    }

    /// Stable Concept IDs are authored state, independent of model/vocabulary releases.
    pub fn auto_tag_rejections(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let mut result = std::collections::HashMap::<String, Vec<String>>::new();
        for entry in self.db.begin_read()?.open_table(REJECTIONS)?.iter()? {
            let (key, _) = entry?;
            let (photo, concept) = key.value();
            result
                .entry(photo.to_owned())
                .or_default()
                .push(concept.to_owned());
        }
        Ok(result)
    }

    pub fn auto_tag_rejections_for(&self, id: &str) -> Result<Vec<String>> {
        self.db
            .begin_read()?
            .open_table(REJECTIONS)?
            .range((id, "")..=(id, "\u{10ffff}"))?
            .map(|entry| Ok(entry?.0.value().1.to_owned()))
            .collect()
    }

    pub fn set_auto_tags_enabled(&self, ids: &[&str], enabled: bool) -> Result<()> {
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        {
            let mut table = write.open_table(OFF)?;
            for id in ids {
                if enabled {
                    table.remove(*id)?;
                } else {
                    table.insert(*id, 1)?;
                }
            }
        }
        write.commit()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredAutoTag {
    pub concept: String,
    pub label: String,
    pub confidence: f32,
    pub displayed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MaterializedTags {
    pub enabled: bool,
    pub model_id: String,
    pub vocabulary_version: String,
    pub thumbnail_version: u32,
    pub tags: Vec<StoredAutoTag>,
}

#[derive(Serialize, Deserialize)]
pub struct ConceptVectors {
    pub scale: f32,
    pub bias: f32,
    pub vectors: std::collections::HashMap<String, Vec<f32>>,
}

impl EmbeddingStore {
    pub(super) fn initialize_auto_tags(&self) -> Result<()> {
        let write = self.db.begin_write()?;
        write.open_table(CONCEPTS)?;
        write.open_table(TAGS)?;
        write.open_table(LEARNING)?;
        write.commit()?;
        Ok(())
    }

    pub fn concept_vectors(&self, model: &str, vocabulary: &str) -> Result<Option<ConceptVectors>> {
        self.db
            .begin_read()?
            .open_table(CONCEPTS)?
            .get((model, vocabulary))?
            .map(|bytes| serde_json::from_slice(bytes.value()))
            .transpose()
            .map_err(Into::into)
    }

    pub fn put_concept_vectors(
        &self,
        model: &str,
        vocabulary: &str,
        vectors: &ConceptVectors,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(vectors)?;
        let write = self.db.begin_write()?;
        write
            .open_table(CONCEPTS)?
            .insert((model, vocabulary), bytes.as_slice())?;
        write.commit()?;
        Ok(())
    }

    pub fn put_learning(
        &self,
        model: &str,
        vocabulary: &str,
        learning: &crate::operations::autotag::learning::Learning,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(learning)?;
        let write = self.db.begin_write()?;
        write
            .open_table(LEARNING)?
            .insert((model, vocabulary), bytes.as_slice())?;
        write.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn learning(
        &self,
        model: &str,
        vocabulary: &str,
    ) -> Result<Option<crate::operations::autotag::learning::Learning>> {
        self.db
            .begin_read()?
            .open_table(LEARNING)?
            .get((model, vocabulary))?
            .map(|bytes| serde_json::from_slice(bytes.value()))
            .transpose()
            .map_err(Into::into)
    }

    pub fn auto_tags(&self, id: &str) -> Result<Option<MaterializedTags>> {
        self.db
            .begin_read()?
            .open_table(TAGS)?
            .get(id)?
            .map(|bytes| serde_json::from_slice(bytes.value()))
            .transpose()
            .map_err(Into::into)
    }

    /// One consistent derived snapshot for owner search and facets.
    pub fn all_auto_tags(&self) -> Result<Vec<(String, MaterializedTags)>> {
        self.db
            .begin_read()?
            .open_table(TAGS)?
            .iter()?
            .map(|entry| {
                let (id, bytes) = entry?;
                Ok((
                    id.value().to_owned(),
                    serde_json::from_slice(bytes.value())?,
                ))
            })
            .collect()
    }

    pub fn put_auto_tags(&self, rows: &[(String, MaterializedTags)]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(TAGS)?;
            for (id, row) in rows {
                let bytes = serde_json::to_vec(row)?;
                table.insert(id.as_str(), bytes.as_slice())?;
            }
        }
        write.commit()?;
        Ok(())
    }
}
