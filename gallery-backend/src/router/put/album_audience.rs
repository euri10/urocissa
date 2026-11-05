use std::{sync::atomic::Ordering, time::Duration};

use arrayvec::ArrayString;
use rocket::serde::{Deserialize, json::Json};

use crate::public::{
    db::{
        tree::{TREE, VERSION_COUNT_TIMESTAMP},
        write_behind::WRITE_BEHIND,
    },
    error::{AppError, ErrorKind},
    structure::audience::Audience,
};
use crate::router::{
    AppResult, GuardResult,
    fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlbumAudienceChange {
    album_id: ArrayString<64>,
    audience: Audience,
    #[serde(default)]
    publish_photos: bool,
}

#[put("/put/album_audience", format = "json", data = "<change>")]
pub async fn set_album_audience(
    auth: GuardResult<GuardAuth>,
    read_only: GuardResult<GuardReadOnlyMode>,
    change: Json<AlbumAudienceChange>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only?;
    let change = change.into_inner();
    if change.publish_photos && change.audience != Audience::Public {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "Publishing photos requires a public album",
        ));
    }
    // Include pending album creation/membership edits before choosing the publication set.
    if !WRITE_BEHIND.flush(Duration::from_secs(30)).await {
        return Err(AppError::new(
            ErrorKind::Conflict,
            "Album edits are still saving; retry publication",
        ));
    }
    tokio::task::spawn_blocking(move || {
        // Match ingestion/deletion's lock order. No membership or identity can change
        // between the zero-pending check and the durable audience transaction.
        let _persistence = TREE
            .persistence_lock
            .lock()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Persistence lock poisoned"))?;
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Tree state lock poisoned"))?;
        if WRITE_BEHIND.status().pending_operations != 0 {
            return Err(AppError::new(
                ErrorKind::Conflict,
                "Album edits changed; retry publication",
            ));
        }
        if state
            .albums
            .get(&change.album_id)
            .is_none_or(|album| album.object.is_trashed)
        {
            return Err(AppError::new(ErrorKind::NotFound, "Album not found"));
        }
        TREE.store
            .set_album_audience(
                change.album_id.as_str(),
                change.audience,
                change.publish_photos,
            )
            .map_err(|error| AppError::from_err(ErrorKind::Database, error))?;
        VERSION_COUNT_TIMESTAMP.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
    .await
    .map_err(|error| AppError::from_err(ErrorKind::Internal, error.into()))?
}
