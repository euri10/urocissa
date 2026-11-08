use rocket::serde::json::Json;
use serde::Deserialize;

use crate::operations::embedding::{self, EmbeddingStatus};
use crate::router::fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode};
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};

#[get("/get/auto-tags/<hash>")]
pub fn get_auto_tags(
    auth: GuardResult<GuardAuth>,
    hash: &str,
) -> AppResult<Json<crate::operations::auto_tags::AutoTagStatus>> {
    let _ = auth?;
    let state = crate::public::db::tree::TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree state lock poisoned"))?;
    let record = state
        .find(hash)
        .and_then(|s| state.get(s))
        .filter(|r| r.object_type == crate::public::structure::object::ObjectType::Image)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo not found"))?;
    let version = record.cache_version;
    drop(state);
    crate::operations::auto_tags::owner_status(hash, version)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        .map(Json)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Auto tags are disabled"))
}

#[get("/get/embedding-status")]
pub fn get_embedding_status(auth: GuardResult<GuardAuth>) -> AppResult<Json<EmbeddingStatus>> {
    let _ = auth?;
    embedding::service()
        .map(|service| Json(service.status()))
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo embeddings are disabled"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseRequest {
    pub paused: bool,
}

#[put("/put/embedding-backfill", format = "json", data = "<request>")]
pub fn pause_embedding_backfill(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
    request: Json<PauseRequest>,
) -> AppResult<Json<EmbeddingStatus>> {
    let _ = auth?;
    let _ = read_only?;
    let service = embedding::service()
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo embeddings are disabled"))?;
    service.pause_backfill(request.into_inner().paused);
    Ok(Json(service.status()))
}

#[cfg(test)]
mod tests;
