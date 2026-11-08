use rocket::serde::{Deserialize, json::Json};

use crate::public::db::tree::TREE;
use crate::public::error::{AppError, ErrorKind};
use crate::public::structure::object::ObjectType;
use crate::router::fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode};
use crate::router::selection::{
    SelectionDescriptor, resolve_selection, resolved_selection_is_current,
};
use crate::router::{AppResult, GuardResult};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditAutoTagsData {
    enabled: bool,
    hash: Option<String>,
    timestamp: Option<i64>,
    selection: Option<SelectionDescriptor>,
    index_array: Option<Vec<u32>>,
}

#[put("/put/edit_auto_tags", format = "json", data = "<json_data>")]
pub async fn edit_auto_tags(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditAutoTagsData>,
) -> AppResult<Json<()>> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || edit(json_data.into_inner()))
        .await
        .map_err(|error| AppError::from_err(ErrorKind::Internal, error.into()))??;
    Ok(Json(()))
}

fn edit(data: EditAutoTagsData) -> AppResult<()> {
    let resolved = if let Some(hash) = data.hash.as_ref() {
        if data.timestamp.is_some() || data.selection.is_some() || data.index_array.is_some() {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Choose either a photo hash or a snapshot selection",
            ));
        }
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppError::new(ErrorKind::InvalidInput, "Invalid photo hash"));
        }
        None
    } else {
        let timestamp = data.timestamp.ok_or_else(|| {
            AppError::new(ErrorKind::InvalidInput, "A selection timestamp is required")
        })?;
        let selection = match (data.selection, data.index_array) {
            (Some(selection), None) => selection,
            (None, Some(indices)) => SelectionDescriptor::explicit(indices),
            _ => {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Exactly one selection descriptor is required",
                ));
            }
        };
        Some(resolve_selection(timestamp, selection)?)
    };

    // Match deletion/media publication lock order so a validated generation cannot
    // be removed and reingested between selection validation and the durable write.
    let persistence = TREE
        .persistence_lock
        .lock()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree persistence lock poisoned"))?;
    let state = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree state lock poisoned"))?;
    let slots = if let Some(resolved) = resolved {
        if !resolved_selection_is_current(
            &state,
            resolved.identity_epoch,
            resolved.selection_epoch,
            &resolved.targets,
        ) {
            return Err(AppError::new(ErrorKind::Conflict, "selection is stale"));
        }
        resolved.targets.iter().collect::<Vec<_>>()
    } else {
        vec![
            state
                .find(data.hash.as_deref().unwrap())
                .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo not found"))?,
        ]
    };
    let hashes = slots
        .into_iter()
        .map(|slot| {
            let record = state
                .get(slot)
                .ok_or_else(|| AppError::new(ErrorKind::Conflict, "selection is stale"))?;
            if record.object_type == ObjectType::Album {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Auto tags cannot be applied to albums",
                ));
            }
            Ok((record.object_type == ObjectType::Image).then_some(record.id.as_str()))
        })
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    TREE.store
        .set_auto_tags_enabled(&hashes, data.enabled)
        .map_err(|error| AppError::from_err(ErrorKind::Database, error))?;
    drop(state);
    drop(persistence);
    crate::operations::embedding::wake();
    Ok(())
}
