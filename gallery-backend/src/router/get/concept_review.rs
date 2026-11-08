//! Owner-only review uses ordinary gallery snapshots, rows, selections and media tokens.
use rocket::serde::json::Json;

use super::get_prefetch::{
    PrefetchReturn, build_random_snapshot, create_json_response, insert_data_into_tree_snapshot,
};
use crate::{
    operations::{embedding, tag_feedback, visibility::PhotoVisibility},
    public::{
        db::tree::TREE,
        error::{AppError, ErrorKind},
    },
    router::AppResult,
};

pub(super) fn execute(
    requested: &str,
    locate: Option<&String>,
    visibility: &PhotoVisibility,
) -> AppResult<Json<PrefetchReturn>> {
    let id = tag_feedback::concept_id(requested);
    let (concept, label) = tag_feedback::concept_choices()?
        .into_iter()
        .find(|(concept, _)| concept == requested || concept == &id)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Unknown Concept"))?;
    let tags = embedding::service()
        .and_then(|service| service.auto_tags.get())
        .ok_or_else(|| {
            AppError::new(
                ErrorKind::Unavailable,
                "Auto tags are not ready. Try again later.",
            )
        })?;
    let review = tags
        .review(concept, label)
        .map_err(|error| AppError::from_err(ErrorKind::Database, error))?;
    let tree = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "Cannot read the photo library"))?;
    let matches = review
        .photos
        .iter()
        .filter_map(|(id, version, _)| {
            tree.find(id).filter(|slot| {
                tree.get(*slot)
                    .is_some_and(|record| record.cache_version == *version)
            })
        })
        .collect();
    let locate = locate.and_then(|id| tree.find(id));
    let (snapshot, locate) = build_random_snapshot(
        matches,
        tree.structural_epoch(),
        tree.identity_epoch(),
        tree.selection_epoch(),
        tree.arena.capacity(),
        locate,
    )?;
    drop(tree);
    let (timestamp, length) = insert_data_into_tree_snapshot(snapshot);
    let mut response =
        create_json_response(timestamp, locate, length, None, None, visibility, false);
    response.concept_review = Some(review.summary);
    Ok(response)
}
