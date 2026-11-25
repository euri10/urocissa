use rocket::serde::json::Json;
use serde::Serialize;

use crate::{
    operations::{suggestions, tag_feedback},
    public::{db::tree::TREE, structure::object::ObjectType},
    router::{AppError, AppResult, ErrorKind, GuardResult, fairing::guard_auth::GuardAuth},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedStatus {
    suggested_tags: Vec<String>,
    pending: bool,
    enabled: bool,
}

#[get("/get/suggested-tags/<hash>")]
pub fn get(auth: GuardResult<GuardAuth>, hash: &str) -> AppResult<Json<SuggestedStatus>> {
    let _ = auth?;
    let state = TREE
        .state
        .read()
        .map_err(|_| AppError::new(ErrorKind::Internal, "tree unavailable"))?;
    let version = state
        .find(hash)
        .and_then(|slot| state.get(slot))
        .filter(|record| record.object_type == ObjectType::Image)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo not found"))?
        .cache_version;
    drop(state);
    let service = suggestions::service()
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Suggested tags are disabled"))?;
    let enabled = TREE.store.auto_tag_settings()?.enabled(hash)?;
    let row = if enabled {
        service.current(hash, version)?
    } else {
        None
    };
    let pending = enabled && row.is_none();
    let mut labels = row.map_or_else(Vec::new, |row| row.labels);
    let decided = tag_feedback::candidate_review_states(hash, &labels)?;
    labels.retain(|label| !decided.contains_key(label));
    Ok(Json(SuggestedStatus {
        suggested_tags: labels,
        pending,
        enabled,
    }))
}

#[get("/get/suggested-tag-status")]
pub fn status(auth: GuardResult<GuardAuth>) -> AppResult<Json<suggestions::Status>> {
    let _ = auth?;
    suggestions::service()
        .map(|service| Json(service.status()))
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Suggested tags are disabled"))
}
