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
        semantic::{self, SearchQuery, Selection, SemanticSearch},
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
    query: &SearchQuery,
    filter: &Expression,
    visibility: &PhotoVisibility,
) -> AppResult<Selection> {
    // Network and redb reads finish before taking the live gallery lock.
    let text_vector = match query {
        SearchQuery::Text(text) => Some(search.query_vector(text)?),
        SearchQuery::Similar(_) => None,
    };
    let (model_id, dim) = if let Some(vector) = &text_vector {
        (vector.model_id.clone(), vector.embedding.len())
    } else {
        // Image-to-image search needs no sidecar request, including during an outage.
        let model = search.store.model()?.ok_or_else(not_processed)?;
        model.validate().map_err(|_| not_processed())?;
        (model.model_id, model.embedding_dim)
    };
    let embeddings = search.store.embeddings(&model_id)?;
    let tree = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "Cannot read the photo library"))?;
    let source = match query {
        SearchQuery::Similar(id) => Some(id.as_str()),
        SearchQuery::Text(_) => None,
    };
    let vector = if let Some(id) = source {
        let slot = tree
            .find(id)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Source photo not found"))?;
        let record = tree.get(slot).unwrap();
        if record.object_type != ObjectType::Image || !visibility.allows_cached(&tree, slot)? {
            return Err(AppError::new(ErrorKind::NotFound, "Source photo not found"));
        }
        &embeddings
            .get(id)
            .filter(|stored| {
                stored.thumbnail_version == record.cache_version
                    && check_vector(&stored.vector, dim).is_ok()
            })
            .ok_or_else(not_processed)?
            .vector
    } else {
        &text_vector.as_ref().unwrap().embedding
    };
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
        if source == Some(record.id.as_str()) {
            continue;
        }
        let Some(stored) = embeddings.get(record.id.as_str()).filter(|stored| {
            stored.thumbnail_version == record.cache_version
                && check_vector(&stored.vector, dim).is_ok()
        }) else {
            pending += 1;
            continue;
        };
        indexed += 1;
        let score = stored
            .vector
            .iter()
            .zip(vector)
            .map(|(left, right)| left * right)
            .sum();
        scores.push(((record.id, record.cache_version), score));
    }
    Ok(search.remember(expression.clone(), semantic::rank(scores), indexed, pending))
}

fn not_processed() -> AppError {
    AppError::new(
        ErrorKind::Conflict,
        "This photo is not processed yet. Try Similar photos again after photo indexing completes.",
    )
}

pub(super) fn execute(
    expression: &Expression,
    sort: GallerySortOrder,
    locate: Option<&String>,
    selection_id: Option<&str>,
    visibility: &PhotoVisibility,
) -> AppResult<Json<PrefetchReturn>> {
    let (query, filter) = semantic::split_expression(expression)?;
    let search = embedding::service().and_then(|service| service.search.get())
        .ok_or_else(|| AppError::new(ErrorKind::Unavailable, "Semantic search is disabled or the photo index is not ready. Other searches still work."))?;
    let selection = if let Some(id) = selection_id {
        search.resume(id, expression)?
    } else {
        select(search, expression, &query, &filter, visibility)?
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
