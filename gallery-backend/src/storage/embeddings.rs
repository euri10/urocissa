//! Disposable embedding cache, deliberately separate from the authored V6 database.
use std::{collections::HashMap, path::Path};

use anyhow::Result;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::operations::embedding::client::ModelInfo;

const EMBEDDINGS: TableDefinition<(&str, &str, u32), &[u8]> = TableDefinition::new("embeddings_v1");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata_v1");
pub const FILE_NAME: &str = "embeddings.redb";

#[derive(Debug, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct StoredEmbedding {
    pub thumbnail_version: u32,
    pub vector: Vec<f32>,
}

pub struct EmbeddingStore {
    db: Database,
}

impl EmbeddingStore {
    pub fn open(path: &Path) -> Result<Self> {
        let mut builder = Database::builder();
        builder.set_cache_size(16 * 1024 * 1024);
        let db = builder.create(path)?;
        let write = db.begin_write()?;
        write.open_table(EMBEDDINGS)?;
        write.open_table(META)?;
        write.commit()?;
        Ok(Self { db })
    }

    pub fn model(&self) -> Result<Option<ModelInfo>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(META)?;
        table
            .get("model")?
            .map(|value| serde_json::from_slice(value.value()))
            .transpose()
            .map_err(Into::into)
    }

    pub fn set_model(&self, model: &ModelInfo) -> Result<()> {
        let bytes = serde_json::to_vec(model)?;
        let write = self.db.begin_write()?;
        write.open_table(META)?.insert("model", bytes.as_slice())?;
        write.commit()?;
        Ok(())
    }

    /// Inventory only; called during reconciliation, never once per candidate.
    pub fn versions(&self, model: &str) -> Result<HashMap<String, u32>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(EMBEDDINGS)?;
        let mut versions = HashMap::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let (item, stored_model, frame) = key.value();
            if stored_model == model && frame == 0 {
                let embedding: StoredEmbedding = bitcode::decode(value.value())?;
                versions.insert(item.to_owned(), embedding.thumbnail_version);
            }
        }
        Ok(versions)
    }

    pub fn put(
        &self,
        item: &str,
        model: &str,
        frame: u32,
        embedding: &StoredEmbedding,
    ) -> Result<()> {
        let bytes = bitcode::encode(embedding);
        let write = self.db.begin_write()?;
        write
            .open_table(EMBEDDINGS)?
            .insert((item, model, frame), bytes.as_slice())?;
        write.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn get(&self, item: &str, model: &str, frame: u32) -> Result<Option<StoredEmbedding>> {
        let read = self.db.begin_read()?;
        let table = read.open_table(EMBEDDINGS)?;
        table
            .get((item, model, frame))?
            .map(|value| bitcode::decode(value.value()))
            .transpose()
            .map_err(Into::into)
    }
}
