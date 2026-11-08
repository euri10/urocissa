//! Durable owner decisions. Fold pending edits into one transaction before publishing indexes.
use std::{collections::BTreeSet, sync::atomic::Ordering};

use crate::{
    public::{
        db::{
            tree::{TREE, VERSION_COUNT_TIMESTAMP},
            write_behind::WRITE_BEHIND,
        },
        error::{AppError, ErrorKind},
        structure::{abstract_data::AbstractData, object::next_mutation_timestamp},
    },
    router::{
        AppResult,
        selection::{ResolvedSelection, resolved_selection_is_current},
    },
};

use super::autotag::{BuiltinCatalog, normalize_name};

static CATALOG: std::sync::LazyLock<BuiltinCatalog> = std::sync::LazyLock::new(|| {
    BuiltinCatalog::embedded().expect("embedded Concept catalog is validated")
});

pub fn concepts_for_tags(tags: &BTreeSet<String>) -> Vec<String> {
    let catalog = &*CATALOG;
    catalog
        .concepts
        .iter()
        .filter(|concept| {
            tags.iter().any(|tag| {
                let name = normalize_name(tag);
                std::iter::once(&concept.label_fr)
                    .chain(std::iter::once(&concept.label_en))
                    .chain(concept.synonyms.iter())
                    .any(|alias| normalize_name(alias) == name)
            })
        })
        .map(|c| c.id.clone())
        .collect()
}

pub fn edit(
    resolved: &ResolvedSelection,
    add: &BTreeSet<String>,
    remove: &BTreeSet<String>,
    rejection: Option<&str>,
) -> AppResult<usize> {
    let removals = remove
        .iter()
        .map(|tag| {
            (
                tag.clone(),
                concepts_for_tags(&BTreeSet::from([tag.clone()])),
            )
        })
        .collect::<Vec<_>>();
    let _persistence = TREE
        .persistence_lock
        .lock()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree persistence lock poisoned"))?;
    let mut state = TREE
        .state
        .write()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree state lock poisoned"))?;
    if !resolved_selection_is_current(
        &state,
        resolved.identity_epoch,
        resolved.selection_epoch,
        &resolved.targets,
    ) {
        return Err(AppError::new(ErrorKind::Conflict, "selection is stale"));
    }
    let targets = &resolved.targets;
    let changed_at = next_mutation_timestamp();
    let mut albums = BTreeSet::new();
    TREE.store
        .write(|writer| {
            for slot in targets.iter() {
                let id = state.get(slot).expect("validated slot").id;
                let durable = writer
                    .get(id.as_str())?
                    .map(crate::storage::store::RecordValue::into_value);
                let mut record = WRITE_BEHIND
                    .logical_record_for_slot(Some(slot), id.as_str(), durable)
                    .ok_or_else(|| anyhow::anyhow!("selected record is missing"))?;
                if record.is_image() {
                    // An intentional removal also suppresses future model/vocabulary guesses,
                    // including while tagging is disabled or its derived cache is unavailable.
                    for (tag, concepts) in &removals {
                        if record.tag().contains(tag) {
                            for concept in concepts {
                                writer.reject_auto_tag(id.as_str(), concept)?;
                            }
                        }
                    }
                    if let Some(concept) = rejection {
                        writer.reject_auto_tag(id.as_str(), concept)?;
                    }
                }
                record.tag_mut().extend(add.iter().cloned());
                for tag in remove {
                    record.tag_mut().remove(tag);
                }
                record.touch_update_at(changed_at);
                if matches!(record, AbstractData::Album(_)) {
                    albums.insert(id);
                }
                writer.insert(&record)?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .map_err(|error| AppError::from_err(ErrorKind::Database, error))?;
    WRITE_BEHIND.cancel_published_media(targets, &albums);
    let universe = state.arena.capacity();
    state
        .query
        .edit_tags(targets.ordinals(), add, remove, universe);
    state.edit_cached_album_objects(targets, changed_at, |object| {
        object.tags.extend(add.iter().cloned());
        for tag in remove {
            object.tags.remove(tag);
        }
    });
    VERSION_COUNT_TIMESTAMP.fetch_add(1, Ordering::Relaxed);
    Ok(resolved.len)
}
