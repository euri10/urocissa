use std::path::PathBuf;

use anyhow::Result;

use crate::public::{
    constant::storage::get_data_path,
    db::tree::TREE,
    structure::{
        abstract_data::{VIEWER_TIER, thumbnail_relative_path},
        object::ObjectType,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photo {
    pub id: String,
    pub thumbnail_version: u32,
    pub thumbnail: PathBuf,
}

/// A snapshot is small metadata only; thumbnail reads and inference never hold the tree lock.
pub trait PhotoCatalog: Send + Sync {
    fn photos(&self) -> Result<Vec<Photo>>;
    fn is_current(&self, photo: &Photo) -> Result<bool>;
}

pub struct GalleryCatalog;

impl PhotoCatalog for GalleryCatalog {
    fn photos(&self) -> Result<Vec<Photo>> {
        let state = TREE
            .state
            .read()
            .map_err(|_| anyhow::anyhow!("tree lock poisoned"))?;
        Ok(state
            .order
            .iter()
            .filter_map(|slot| state.get(*slot))
            .filter(|record| record.object_type == ObjectType::Image)
            .map(|record| Photo {
                id: record.id.to_string(),
                thumbnail_version: record.cache_version,
                thumbnail: get_data_path().join(thumbnail_relative_path(
                    VIEWER_TIER,
                    record.id.as_str(),
                    record.cache_version,
                )),
            })
            .collect())
    }

    fn is_current(&self, photo: &Photo) -> Result<bool> {
        let state = TREE
            .state
            .read()
            .map_err(|_| anyhow::anyhow!("tree lock poisoned"))?;
        Ok(state
            .find(&photo.id)
            .and_then(|slot| state.get(slot))
            .is_some_and(|record| {
                record.object_type == ObjectType::Image
                    && record.cache_version == photo.thumbnail_version
            }))
    }
}
