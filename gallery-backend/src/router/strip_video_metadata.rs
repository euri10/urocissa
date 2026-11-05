use rocket::http::Status;
use rocket::response::status::Custom;
use rocket::serde::json::Json;

use crate::public::constant::storage::get_data_path;
use crate::router::fairing::guard_auth::GuardAuth;
use crate::router::fairing::guard_read_only_mode::GuardReadOnlyMode;
use crate::router::{AppError, AppResult, ErrorKind, GuardResult};
use crate::tasks::actor::strip_video_metadata::{
    StripJobAccepted, StripJobStatus, start_strip_job, strip_job_status,
};

#[post("/post/video/strip-metadata")]
pub fn start_video_metadata_strip(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Custom<Json<StripJobAccepted>>> {
    let _ = auth?;
    let _ = read_only?;
    let accepted = start_strip_job(get_data_path().clone()).ok_or_else(|| {
        AppError::new(
            ErrorKind::Conflict,
            "A compressed-video metadata stripping job is already running",
        )
    })?;
    Ok(Custom(Status::Accepted, Json(accepted)))
}

#[get("/get/video/strip-metadata")]
pub fn get_video_metadata_strip(
    auth: GuardResult<GuardAuth>,
) -> AppResult<Json<Option<StripJobStatus>>> {
    let _ = auth?;
    Ok(Json(strip_job_status()))
}
