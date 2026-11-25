//! Disposable Florence results, separate from both authored feedback and Auto tags.
use std::{path::Path, sync::Arc};

use anyhow::Result;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

const ROWS: TableDefinition<&str, &[u8]> = TableDefinition::new("suggested_tags_v1");
pub const FILE_NAME: &str = "suggestions.redb";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuggestedTags {
    pub version: String,
    pub thumbnail_version: u32,
    pub labels: Vec<String>,
}

#[derive(Clone)]
pub struct SuggestionStore(Arc<Database>);

impl SuggestionStore {
    pub fn open(path: &Path) -> Result<Self> {
        let mut builder = Database::builder();
        builder.set_cache_size(4 * 1024 * 1024);
        let db = builder.create(path)?;
        let write = db.begin_write()?;
        write.open_table(ROWS)?;
        write.commit()?;
        Ok(Self(Arc::new(db)))
    }

    pub fn get(&self, id: &str) -> Result<Option<SuggestedTags>> {
        self.0
            .begin_read()?
            .open_table(ROWS)?
            .get(id)?
            .map(|row| serde_json::from_slice(row.value()))
            .transpose()
            .map_err(Into::into)
    }

    pub fn all(&self) -> Result<Vec<(String, SuggestedTags)>> {
        self.0
            .begin_read()?
            .open_table(ROWS)?
            .iter()?
            .map(|row| {
                let (id, bytes) = row?;
                Ok((
                    id.value().to_owned(),
                    serde_json::from_slice(bytes.value())?,
                ))
            })
            .collect()
    }

    pub fn put(&self, id: &str, row: &SuggestedTags) -> Result<()> {
        let bytes = serde_json::to_vec(row)?;
        let write = self.0.begin_write()?;
        write.open_table(ROWS)?.insert(id, bytes.as_slice())?;
        write.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_nonempty_results_survive_reopening() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join(FILE_NAME);
        let row = SuggestedTags {
            version: "frozen".into(),
            thumbnail_version: 2,
            labels: vec![],
        };
        {
            let store = SuggestionStore::open(&path)?;
            store.put("one", &row)?;
            store.put(
                "two",
                &SuggestedTags {
                    labels: vec!["dog".into()],
                    ..row.clone()
                },
            )?;
        }
        let store = SuggestionStore::open(&path)?;
        assert_eq!(store.get("one")?, Some(row));
        assert_eq!(store.all()?.len(), 2);
        Ok(())
    }
}
