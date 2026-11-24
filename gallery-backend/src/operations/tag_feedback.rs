//! Durable owner decisions. Fold pending edits into one transaction before publishing indexes.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::Ordering,
};

use anyhow::Context;

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

use super::autotag::{
    BuiltinCatalog, Vocabulary,
    learning::{personal_id, personal_names},
};

static VOCABULARY: std::sync::LazyLock<Vocabulary> = std::sync::LazyLock::new(|| {
    BuiltinCatalog::embedded()
        .and_then(|catalog| catalog.alias_vocabulary())
        .expect("embedded Concept catalog is validated")
});

pub fn concepts_for_tags(tags: &BTreeSet<String>) -> Vec<String> {
    tags.iter()
        .filter(|tag| !tag.trim().is_empty())
        .map(|tag| concept_id(tag))
        .collect()
}

pub fn concept_id(tag: &str) -> String {
    VOCABULARY
        .concept_named(tag)
        .map_or_else(|| personal_id(tag), |c| c.id.clone())
}

fn canonical_rejections(ids: Vec<String>) -> Vec<String> {
    let mut ids = ids
        .into_iter()
        .map(|id| VOCABULARY.canonical_rejection(&id).to_owned())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

pub fn rejections_for(photo: &str) -> anyhow::Result<Vec<String>> {
    Ok(canonical_rejections(
        TREE.store.auto_tag_rejections_for(photo)?,
    ))
}

pub fn all_rejections() -> anyhow::Result<std::collections::HashMap<String, Vec<String>>> {
    Ok(TREE
        .store
        .auto_tag_rejections()?
        .into_iter()
        .map(|(photo, ids)| (photo, canonical_rejections(ids)))
        .collect())
}

fn rejected_by(
    writer: &crate::storage::store::RecordWriter<'_>,
    photo: &str,
    concept: &str,
) -> anyhow::Result<bool> {
    for key in VOCABULARY.rejection_keys(concept) {
        if writer.auto_tag_rejected(photo, &key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_rejection(
    writer: &mut crate::storage::store::RecordWriter<'_>,
    photo: &str,
    concept: &str,
    clear: bool,
) -> anyhow::Result<()> {
    for key in VOCABULARY.rejection_keys(concept) {
        if clear {
            writer.clear_auto_tag_rejection(photo, &key)?;
        } else {
            writer.reject_auto_tag(photo, &key)?;
        }
    }
    Ok(())
}

/// Read authored feedback without creating decisions for unreviewed proposals.
pub fn candidate_review_states(
    hash: &str,
    labels: &[String],
) -> anyhow::Result<BTreeMap<String, &'static str>> {
    let _persistence = TREE
        .persistence_lock
        .lock()
        .map_err(|_| anyhow::anyhow!("tree persistence lock poisoned"))?;
    let state = TREE
        .state
        .read()
        .map_err(|_| anyhow::anyhow!("tree state lock poisoned"))?;
    let slot = state.find(hash).context("photo unavailable")?;
    let durable = TREE
        .store
        .reader()?
        .get(hash)?
        .map(crate::storage::store::RecordValue::into_value);
    let record = WRITE_BEHIND
        .logical_record_for_slot(Some(slot), hash, durable)
        .context("photo unavailable")?;
    let confirmed: BTreeSet<_> = record.tag().iter().map(|tag| concept_id(tag)).collect();
    let rejected = rejections_for(hash)?;
    Ok(labels
        .iter()
        .filter_map(|label| {
            let id = concept_id(label);
            let decision = if confirmed.contains(&id) {
                "confirmed"
            } else if rejected.contains(&id) {
                "rejected"
            } else {
                return None;
            };
            Some((label.clone(), decision))
        })
        .collect())
}

/// Choices come from authored vocabulary, so feedback works while the tagger is offline.
pub fn concept_choices() -> AppResult<Vec<(String, String)>> {
    let state = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree state lock poisoned"))?;
    let personal = personal_names(
        &VOCABULARY,
        state.query.tags.iter().filter_map(|(tag, members)| {
            members
                .iter()
                .any(|ordinal| {
                    state
                        .slot_for_ordinal(ordinal)
                        .and_then(|s| state.get(s))
                        .is_some_and(|r| {
                            r.object_type == crate::public::structure::object::ObjectType::Image
                        })
                })
                .then_some(tag.as_str())
        }),
    );
    Ok(VOCABULARY
        .concepts()
        .iter()
        .map(|c| (c.id.clone(), c.label.clone()))
        .chain(personal)
        .collect())
}

pub fn edit(
    resolved: &ResolvedSelection,
    add: &BTreeSet<String>,
    remove: &BTreeSet<String>,
    rejection: Option<&str>,
) -> AppResult<usize> {
    edit_inner(resolved, add, remove, rejection, None)
}

#[derive(Clone, Copy)]
pub enum CandidateAction {
    Confirm,
    Reject,
    ClearRejection,
}

/// An explicit reviewed candidate can introduce a personal Concept without having been an Auto tag.
pub fn edit_candidate(
    resolved: &ResolvedSelection,
    label: &str,
    action: CandidateAction,
) -> AppResult<usize> {
    let concept = concept_id(label);
    let add = match action {
        CandidateAction::Confirm => BTreeSet::from([label.to_owned()]),
        CandidateAction::Reject | CandidateAction::ClearRejection => BTreeSet::new(),
    };
    let rejection = matches!(action, CandidateAction::Reject).then_some(concept.as_str());
    edit_inner(
        resolved,
        &add,
        &BTreeSet::new(),
        rejection,
        Some((&concept, action)),
    )
}

#[derive(Debug)]
struct CandidateConflict;
impl std::fmt::Display for CandidateConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("This decision conflicts with an existing User tag or Rejection")
    }
}
impl std::error::Error for CandidateConflict {}

fn candidate_conflicts(
    record: &AbstractData,
    writer: &crate::storage::store::RecordWriter<'_>,
    concept: &str,
    action: CandidateAction,
    add: &BTreeSet<String>,
) -> anyhow::Result<bool> {
    Ok(match action {
        CandidateAction::Confirm => {
            rejected_by(writer, record.hash().as_str(), concept)?
                || record
                    .tag()
                    .iter()
                    .any(|tag| concept_id(tag) == concept && !add.contains(tag))
        }
        CandidateAction::Reject => record.tag().iter().any(|tag| concept_id(tag) == concept),
        CandidateAction::ClearRejection => false,
    })
}

fn edit_inner(
    resolved: &ResolvedSelection,
    add: &BTreeSet<String>,
    remove: &BTreeSet<String>,
    rejection: Option<&str>,
    candidate: Option<(&str, CandidateAction)>,
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
                    if let Some((concept, action)) = candidate
                        && candidate_conflicts(&record, writer, concept, action, add)?
                    {
                        return Err(CandidateConflict.into());
                    }
                    if let Some((concept, CandidateAction::ClearRejection)) = candidate {
                        write_rejection(writer, id.as_str(), concept, true)?;
                    }
                    // An intentional removal also suppresses future model/vocabulary guesses,
                    // including while tagging is disabled or its derived cache is unavailable.
                    for (tag, concepts) in &removals {
                        if record.tag().contains(tag) {
                            for concept in concepts {
                                write_rejection(writer, id.as_str(), concept, false)?;
                            }
                        }
                    }
                    if let Some(concept) = rejection {
                        write_rejection(writer, id.as_str(), concept, false)?;
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
        .map_err(|error| {
            if error.is::<CandidateConflict>() {
                AppError::new(
                    ErrorKind::Conflict,
                    "Decision conflicts with an existing User tag or Rejection",
                )
            } else {
                AppError::from_err(ErrorKind::Database, error)
            }
        })?;
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

#[cfg(test)]
mod rejection_tests;
