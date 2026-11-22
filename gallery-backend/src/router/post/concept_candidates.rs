use crate::{
    public::error::{AppError, ErrorKind},
    router::{AppResult, GuardResult, fairing::guard_auth::GuardAuth},
};
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct CandidateRequest {
    hash: String,
}

#[derive(Serialize)]
pub struct CandidateResponse {
    candidates: Vec<String>,
    decisions: BTreeMap<String, &'static str>,
}

#[post("/post/concept_candidates", format = "json", data = "<data>")]
pub async fn candidates(
    auth: GuardResult<GuardAuth>,
    data: Json<CandidateRequest>,
) -> AppResult<Json<CandidateResponse>> {
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let candidates =
            crate::operations::concept_candidates::for_photo(&data.hash).map_err(|_| {
                AppError::new(
                    ErrorKind::Unavailable,
                    "Local suggestions are unavailable for this photo.",
                )
            })?;
        let decisions =
            crate::operations::tag_feedback::candidate_review_states(&data.hash, &candidates)
                .map_err(|_| {
                    AppError::new(
                        ErrorKind::Unavailable,
                        "Local suggestions are unavailable for this photo.",
                    )
                })?;
        Ok(Json(CandidateResponse {
            candidates,
            decisions,
        }))
    })
    .await
    .map_err(|_| AppError::new(ErrorKind::Unavailable, "Local suggestions are unavailable."))?
}
