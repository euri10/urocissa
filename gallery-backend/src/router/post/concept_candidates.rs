use crate::{
    public::error::{AppError, ErrorKind},
    router::{AppResult, GuardResult, fairing::guard_auth::GuardAuth},
};
use rocket::serde::{Deserialize, Serialize, json::Json};

#[derive(Deserialize)]
pub struct CandidateRequest {
    hash: String,
}

#[derive(Serialize)]
pub struct CandidateResponse {
    candidates: Vec<String>,
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
        Ok(Json(CandidateResponse { candidates }))
    })
    .await
    .map_err(|_| AppError::new(ErrorKind::Unavailable, "Local suggestions are unavailable."))?
}
