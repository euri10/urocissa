//! Adapt owner-only semantic results to the existing compact gallery snapshot format.
use std::collections::HashMap;

use rand::seq::SliceRandom;
use rocket::serde::json::Json;

use super::get_prefetch::{
    PrefetchReturn, build_random_snapshot, create_json_response, insert_data_into_tree_snapshot,
};
use crate::{
    operations::{
        autotag::check_vector,
        embedding,
        semantic::{self, Selection, SemanticSearch},
        visibility::PhotoVisibility,
    },
    public::{
        db::{
            tree::{
                TREE,
                state::{CompiledExpression, SlotRef, TreeState},
            },
            tree_snapshot::read_scrollbar::build_scrollbar,
        },
        error::{AppError, ErrorKind},
        structure::{
            expression::Expression, gallery_sort_order::GallerySortOrder, object::ObjectType,
        },
    },
    router::AppResult,
};

fn eligible(
    tree: &TreeState,
    slot: SlotRef,
    filter: &CompiledExpression<'_>,
    audience_filtered: bool,
    visibility: &PhotoVisibility,
) -> AppResult<bool> {
    let Some(record) = tree.get(slot) else {
        return Ok(false);
    };
    let audience = audience_filtered
        .then(|| visibility.audience(record.id.as_str()))
        .transpose()?;
    Ok(record.object_type == ObjectType::Image
        && visibility.allows_cached(tree, slot)?
        && filter.matches(record, slot.index(), audience))
}

fn select(
    search: &SemanticSearch,
    expression: &Expression,
    text: &str,
    filter: &Expression,
    visibility: &PhotoVisibility,
) -> AppResult<Selection> {
    // Network and redb reads finish before taking the live gallery lock.
    let query = search.query_vector(text)?;
    let embeddings = search.store.embeddings(&query.model_id)?;
    let tree = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "Cannot read the photo library"))?;
    let compiled = tree.compile_expression(filter, None, false);
    let audience_filtered = expression.contains_audience();
    let mut scores = Vec::new();
    let mut indexed = 0;
    let mut pending = 0;
    for &slot in tree.order.iter() {
        if !eligible(&tree, slot, &compiled, audience_filtered, visibility)? {
            continue;
        }
        let record = tree.get(slot).unwrap();
        let Some(stored) = embeddings.get(record.id.as_str()).filter(|stored| {
            stored.thumbnail_version == record.cache_version
                && check_vector(&stored.vector, query.embedding.len()).is_ok()
        }) else {
            pending += 1;
            continue;
        };
        indexed += 1;
        let score = stored
            .vector
            .iter()
            .zip(&query.embedding)
            .map(|(left, right)| left * right)
            .sum();
        scores.push(((record.id, record.cache_version), score));
    }
    Ok(search.remember(expression.clone(), semantic::rank(scores), indexed, pending))
}

pub(super) fn execute(
    expression: &Expression,
    sort: GallerySortOrder,
    locate: Option<&String>,
    selection_id: Option<&str>,
    visibility: &PhotoVisibility,
) -> AppResult<Json<PrefetchReturn>> {
    let (text, filter) = semantic::split_expression(expression)?;
    let search = embedding::service().and_then(|service| service.search.get())
        .ok_or_else(|| AppError::new(ErrorKind::Unavailable, "Semantic search is disabled or the photo index is not ready. Other searches still work."))?;
    let selection = if let Some(id) = selection_id {
        search.resume(id, expression)?
    } else {
        select(search, expression, &text, &filter, visibility)?
    };
    let tree = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "Cannot read the photo library"))?;
    let compiled = tree.compile_expression(&filter, None, false);
    let audience_filtered = expression.contains_audience();
    let mut matches = Vec::new();
    for (id, version) in &selection.photos {
        if let Some(slot) = tree.find(id.as_str())
            && eligible(&tree, slot, &compiled, audience_filtered, visibility)?
            && tree
                .get(slot)
                .is_some_and(|record| record.cache_version == *version)
        {
            matches.push(slot);
        }
    }
    match sort {
        GallerySortOrder::Relevance => {}
        GallerySortOrder::Random => matches.shuffle(&mut rand::rng()),
        GallerySortOrder::Ascending | GallerySortOrder::Descending => {
            let positions: HashMap<_, _> = tree
                .order
                .iter()
                .enumerate()
                .map(|(index, slot)| (*slot, index))
                .collect();
            matches.sort_unstable_by_key(|slot| positions[slot]);
            if sort == GallerySortOrder::Ascending {
                matches.reverse();
            }
        }
    }
    let scrollbar = if matches!(
        sort,
        GallerySortOrder::Ascending | GallerySortOrder::Descending
    ) {
        build_scrollbar(
            matches
                .iter()
                .map(|slot| tree.get(*slot).unwrap().timestamp),
        )
    } else {
        Vec::new()
    };
    let locate = locate.and_then(|id| tree.find(id));
    let (mut snapshot, locate) = build_random_snapshot(
        matches,
        tree.structural_epoch(),
        tree.identity_epoch(),
        tree.selection_epoch(),
        tree.arena.capacity(),
        locate,
    )?;
    snapshot.scrollbar = scrollbar;
    drop(tree);
    let (timestamp, length) = insert_data_into_tree_snapshot(snapshot);
    let mut response = create_json_response(
        timestamp,
        locate,
        length,
        None,
        None,
        visibility,
        audience_filtered,
    );
    response.semantic_search = Some(selection.summary);
    Ok(response)
}

#[cfg(test)]
mod tests;
