//! Viewer-specific album presentation never changes the owner's stored cover or counts.
use crate::public::db::tree::{
    TREE,
    state::{CacheRecord, TreeState},
};
use crate::public::error::{AppError, ErrorKind};
use crate::public::structure::{
    abstract_data::AbstractData,
    audience::Viewer,
    expression::{AlbumFilterValue, Expression},
    object::ObjectType,
    response::database_timestamp::DataBaseTimestampReturn,
};
use crate::router::AppResult;

use super::{
    public_metadata::ViewerKind, transitor::abstract_data_to_database_timestamp_return,
    visibility::PhotoVisibility,
};

/// Album filters must not disclose membership of hidden albums, even under NOT/OR.
pub fn visible_album_expression(
    expression: &Expression,
    state: &TreeState,
    visibility: &PhotoVisibility,
) -> AppResult<Expression> {
    if visibility.viewer != Viewer::Anonymous {
        return Ok(expression.clone());
    }
    Ok(match expression {
        Expression::Album(AlbumFilterValue::Value(id)) => {
            if let Some(slot) = state.find(id.as_str())
                && state.albums.contains_key(id)
                && visibility.allows_cached(state, slot)?
            {
                expression.clone()
            } else {
                Expression::Or(vec![])
            }
        }
        Expression::Album(AlbumFilterValue::Exists(exists)) => {
            let mut albums = Vec::new();
            for id in state.albums.keys() {
                if let Some(slot) = state.find(id.as_str())
                    && visibility.allows_cached(state, slot)?
                {
                    albums.push(Expression::Album(AlbumFilterValue::Value(*id)));
                }
            }
            let any = Expression::Or(albums);
            if *exists {
                any
            } else {
                Expression::Not(Box::new(any))
            }
        }
        Expression::Or(items) => Expression::Or(
            items
                .iter()
                .map(|item| visible_album_expression(item, state, visibility))
                .collect::<AppResult<_>>()?,
        ),
        Expression::And(items) => Expression::And(
            items
                .iter()
                .map(|item| visible_album_expression(item, state, visibility))
                .collect::<AppResult<_>>()?,
        ),
        Expression::Not(item) => {
            Expression::Not(Box::new(visible_album_expression(item, state, visibility)?))
        }
        _ => expression.clone(),
    })
}

pub fn visible_data_to_database_timestamp_return(
    mut data: AbstractData,
    timestamp: i64,
    show_download: bool,
    show_metadata: bool,
    viewer: ViewerKind,
    visibility: &PhotoVisibility,
) -> AppResult<DataBaseTimestampReturn> {
    let photo_audience = visibility.owner_audience(&data)?;
    let album_audience = matches!(&data, AbstractData::Album(_))
        .then(|| visibility.audience(data.hash().as_str()))
        .transpose()?;
    if matches!(visibility.viewer, Viewer::Share(_)) {
        visibility.project_album_counts(&mut data)?;
    }
    if visibility.viewer == Viewer::Anonymous {
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Tree state lock poisoned"))?;
        if let AbstractData::Album(album) = &mut data {
            album.metadata.item_count = 0;
            album.metadata.item_size = 0;
            let mut newest = None;
            let mut chosen = None;
            if let Some(members) = state.query.albums.get(&album.object.id) {
                for ordinal in members.iter() {
                    let Some(slot) = state.slot_for_ordinal(ordinal) else {
                        continue;
                    };
                    let Some(record) = state.get(slot) else {
                        continue;
                    };
                    if record.object_type == ObjectType::Album
                        || !visibility.allows_cached(&state, slot)?
                    {
                        continue;
                    }
                    album.metadata.item_count += 1;
                    album.metadata.item_size = album.metadata.item_size.saturating_add(record.size);
                    if album.metadata.cover == Some(record.id) {
                        chosen = Some(record);
                    }
                    if newest.is_none_or(|old: &CacheRecord| {
                        record.timestamp > old.timestamp
                            || (record.timestamp == old.timestamp && record.id < old.id)
                    }) {
                        newest = Some(record);
                    }
                }
            }
            let cover = chosen.or(newest);
            album.metadata.cover = cover.map(|record| record.id);
            album.object.thumbhash = cover.and_then(CacheRecord::thumbhash_vec);
            album.object.cache_version = cover.map_or(0, |record| record.cache_version);
        } else if let Some(albums) = data.albums_mut() {
            let mut visible = std::collections::HashSet::new();
            for id in albums.iter() {
                if let Some(slot) = state.find(id.as_str())
                    && state.albums.contains_key(id)
                    && visibility.allows_cached(&state, slot)?
                {
                    visible.insert(*id);
                }
            }
            *albums = visible;
        }
    }
    let auto_tags = if viewer == ViewerKind::Owner && data.is_image() {
        super::auto_tags::owner_status(data.hash().as_str(), data.cache_version())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
    } else {
        None
    };
    let mut result = abstract_data_to_database_timestamp_return(
        data,
        timestamp,
        show_download,
        show_metadata,
        viewer,
    );
    result.audience = photo_audience;
    if let Some(tags) = auto_tags {
        let fields = serde_json::to_value(tags).expect("Auto tags serialize");
        result
            .abstract_data
            .as_object_mut()
            .expect("record object")
            .extend(fields.as_object().unwrap().clone());
    }
    if let Some(audience) = album_audience {
        result.abstract_data["audience"] =
            serde_json::to_value(audience).expect("audience serializes");
    }
    Ok(result)
}
