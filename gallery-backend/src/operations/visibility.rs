//! The read-path adapter for the pure audience policy. Use this for every photo read.
use crate::public::db::tree::{
    TREE,
    state::{SlotRef, TreeState},
};
use crate::public::error::{AppError, ErrorKind};
use crate::public::structure::{
    abstract_data::AbstractData,
    audience::{Audience, Viewer},
    object::ObjectType,
};
use crate::router::{AppResult, claims::Role};
use crate::storage::audience::PhotoAudiences;

pub fn viewer(role: &Role) -> Viewer {
    match role {
        Role::Admin => Viewer::Owner,
        Role::Public => Viewer::Anonymous,
        Role::Share(share) => Viewer::Share(share.album_id),
    }
}

pub struct PhotoVisibility {
    pub viewer: Viewer,
    audiences: PhotoAudiences,
}

impl PhotoVisibility {
    pub fn new(viewer: Viewer) -> AppResult<Self> {
        Ok(Self {
            viewer,
            audiences: TREE.store.photo_audiences().map_err(database_error)?,
        })
    }

    pub fn revision(&self) -> u64 {
        self.audiences.revision
    }

    pub fn allows_data(&self, data: &AbstractData) -> AppResult<bool> {
        let hash = data.hash();
        let is_album = matches!(data, AbstractData::Album(_));
        let share_member = match self.viewer {
            Viewer::Share(album) => {
                if is_album {
                    hash == album
                } else {
                    data.albums().is_some_and(|albums| albums.contains(&album))
                }
            }
            _ => false,
        };
        self.allows(hash.as_str(), is_album, data.is_trashed(), share_member)
    }

    pub fn allows_cached(&self, state: &TreeState, slot: SlotRef) -> AppResult<bool> {
        let Some(record) = state.get(slot) else {
            return Ok(false);
        };
        let is_album = record.object_type == ObjectType::Album;
        let share_member = match self.viewer {
            Viewer::Share(album) => {
                if is_album {
                    record.id == album
                } else {
                    state
                        .query
                        .albums
                        .get(&album)
                        .is_some_and(|members| members.contains(slot.index()))
                }
            }
            _ => false,
        };
        self.allows(
            record.id.as_str(),
            is_album,
            state.query.trashed.contains(slot.index()),
            share_member,
        )
    }

    fn allows(
        &self,
        hash: &str,
        is_album: bool,
        trashed: bool,
        share_member: bool,
    ) -> AppResult<bool> {
        // Album audience/cover policy is the separate album task (.9).
        let audience = if is_album {
            Audience::Public
        } else {
            self.audiences.get(hash).map_err(database_error)?
        };
        Ok(self.viewer.can_view(audience, trashed, share_member))
    }

    pub fn require_data(&self, data: &AbstractData) -> AppResult<()> {
        if !self.allows_data(data)? {
            return Err(AppError::new(ErrorKind::NotFound, "Item not found"));
        }
        Ok(())
    }

    /// Project member totals for a response without changing stored owner aggregates.
    /// Album authorization and cover policy remain separate from these photo totals.
    pub fn project_album_counts(&self, data: &mut AbstractData) -> AppResult<()> {
        if self.viewer == Viewer::Owner {
            return Ok(());
        }
        let AbstractData::Album(album) = data else {
            return Ok(());
        };
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Tree state lock poisoned"))?;
        album.metadata.item_count = 0;
        album.metadata.item_size = 0;
        for slot in state
            .query
            .albums
            .get(&album.object.id)
            .into_iter()
            .flat_map(|members| members.iter())
            .filter_map(|ordinal| state.slot_for_ordinal(ordinal))
        {
            if self.allows_cached(&state, slot)?
                && let Some(record) = state.get(slot)
                && record.object_type != ObjectType::Album
            {
                album.metadata.item_count += 1;
                album.metadata.item_size += record.size;
            }
        }
        Ok(())
    }

    pub fn require_snapshot_range(
        &self,
        snapshot: &crate::public::db::tree_snapshot::read_tree_snapshot::MyCow,
        start: usize,
        end: usize,
    ) -> AppResult<()> {
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Tree state lock poisoned"))?;
        for index in start..end {
            let slot = snapshot.get_slot_ref(index).map_err(database_error)?;
            if !self.allows_cached(&state, SlotRef::from_raw(slot))? {
                return Err(AppError::new(ErrorKind::NotFound, "Item not found"));
            }
        }
        Ok(())
    }
}

fn database_error(error: anyhow::Error) -> AppError {
    AppError::from_err(ErrorKind::Database, error)
}
