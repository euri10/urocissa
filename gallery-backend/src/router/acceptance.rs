//! Freeze owner-only evaluation samples; judgments never enter training.
use std::{collections::HashSet, sync::atomic::Ordering};

use anyhow::{Result, ensure};
use rand::seq::SliceRandom;
use rocket::serde::json::Json;
use serde::Deserialize;

use crate::{
    operations::{auto_tags, autotag::BuiltinCatalog, embedding, tag_feedback},
    public::{
        db::tree::{TREE, VERSION_COUNT_TIMESTAMP},
        structure::object::ObjectType,
    },
    router::{
        AppError, AppResult, ErrorKind,
        fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode},
    },
    storage::acceptance::{Judgment, ReviewPhoto, ReviewRound, Suggestion},
};

fn failure(error: anyhow::Error) -> AppError {
    // No photo identity, model output or submitted body in logs/errors.
    drop(error);
    AppError::new(
        ErrorKind::Conflict,
        "Review changed or inputs are not ready; refresh and check the protocol",
    )
}

fn photo(hash: &str, role: &str) -> Result<ReviewPhoto> {
    let (version, timestamp) = {
        let state = TREE
            .state
            .read()
            .map_err(|_| anyhow::anyhow!("Library unavailable"))?;
        let item = state
            .find(hash)
            .and_then(|slot| state.get(slot))
            .filter(|item| item.object_type == ObjectType::Image)
            .ok_or_else(|| anyhow::anyhow!("Photo not found"))?;
        (item.cache_version, item.timestamp)
    };
    let record = TREE
        .store
        .reader()?
        .get(hash)?
        .ok_or_else(|| anyhow::anyhow!("Photo missing"))?
        .into_value();
    ensure!(
        !record.is_trashed(),
        "Trash is not an evaluation population"
    );
    let status = auto_tags::owner_status(hash, version)?
        .ok_or_else(|| anyhow::anyhow!("Indexing unavailable"))?;
    Ok(ReviewPhoto {
        hash: hash.to_owned(),
        version,
        timestamp,
        role: role.into(),
        enabled: status.auto_tags_enabled,
        pending: status.auto_tags_pending,
        before: status
            .auto_tags
            .into_iter()
            .filter(|tag| tag.displayed)
            .map(|tag| Suggestion {
                concept: tag.concept,
                label: tag.label,
                judgment: None,
            })
            .collect(),
        after: None,
        seen_before: false,
        seen_after: false,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Start {
    kind: String,
    #[serde(default)]
    teaching: Vec<String>,
    #[serde(default)]
    held_out: Vec<String>,
    #[serde(default)]
    plant: String,
    #[serde(default)]
    separation_confirmed: bool,
    #[serde(default)]
    new_round: bool,
}

fn create(request: &Start) -> Result<ReviewRound> {
    ensure!(
        ["random", "garden"].contains(&request.kind.as_str()),
        "Unknown review kind"
    );
    let service = embedding::service().ok_or_else(|| anyhow::anyhow!("Indexing unavailable"))?;
    let status = service.status();
    let tags = service
        .auto_tags
        .get()
        .ok_or_else(|| anyhow::anyhow!("Tags unavailable"))?;
    let calculation = tags.recalculation_status();
    ensure!(
        status.pending_photos == 0 && calculation.up_to_date,
        "Wait for backfill and learning"
    );
    let revision = VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed);
    let mut ids = TREE
        .store
        .reader()?
        .values()?
        .map(|record| record.map(crate::storage::store::RecordValue::into_value))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|record| record.is_image() && !record.is_trashed())
        .map(|record| record.hash().to_string())
        .collect::<Vec<_>>();
    let population = ids.len();
    let photos = if request.kind == "random" {
        ensure!(ids.len() >= 100, "Need 100 photos");
        ids.shuffle(&mut rand::rng());
        ids.truncate(100);
        ids.iter()
            .map(|id| photo(id, "random"))
            .collect::<Result<Vec<_>>>()?
    } else {
        ensure!(
            (5..=10).contains(&request.teaching.len())
                && (5..=10).contains(&request.held_out.len()),
            "Use 5-10 photos in each split"
        );
        ensure!(
            request.separation_confirmed
                && !request.plant.trim().is_empty()
                && request.plant.len() <= 100,
            "Confirm the held-out protocol and owner plant name"
        );
        let all = request
            .teaching
            .iter()
            .chain(&request.held_out)
            .collect::<HashSet<_>>();
        ensure!(
            all.len() == request.teaching.len() + request.held_out.len(),
            "Splits must be disjoint"
        );
        let concept = tag_feedback::concept_id(request.plant.trim());
        for id in &request.held_out {
            let record = TREE
                .store
                .reader()?
                .get(id)?
                .ok_or_else(|| anyhow::anyhow!("Photo missing"))?
                .into_value();
            ensure!(
                !record
                    .tag()
                    .iter()
                    .any(|tag| tag_feedback::concept_id(tag) == concept)
                    && !TREE.store.auto_tag_rejections_for(id)?.contains(&concept),
                "Held-out target already taught"
            );
        }
        request
            .teaching
            .iter()
            .map(|id| photo(id, "teaching"))
            .chain(request.held_out.iter().map(|id| photo(id, "heldOut")))
            .collect::<Result<Vec<_>>>()?
    };
    ensure!(
        revision == VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed)
            && tags.recalculation_status().last_completed_at == calculation.last_completed_at,
        "Library changed while sampling"
    );
    Ok(ReviewRound {
        id: uuid::Uuid::new_v4().to_string(),
        kind: request.kind.clone(),
        captured_at: chrono::Utc::now().timestamp_millis(),
        model: status.model_id.unwrap_or_default(),
        vocabulary: BuiltinCatalog::embedded()?.version,
        materialized_at: calculation.last_completed_at.unwrap_or_default(),
        library_revision: revision,
        population,
        plant: request.plant.trim().into(),
        separation_confirmed: request.separation_confirmed,
        after_captured_at: None,
        photos,
        feedback: String::new(),
        effort_seconds: 0,
    })
}

#[get("/get/acceptance-review?<kind>")]
pub fn get(_auth: GuardAuth, kind: Option<&str>) -> AppResult<Json<Option<ReviewRound>>> {
    TREE.store
        .acceptance_round(kind.unwrap_or("random"))
        .map(Json)
        .map_err(failure)
}

#[post("/post/acceptance-review", format = "json", data = "<request>")]
pub async fn start(
    _auth: GuardAuth,
    _writable: GuardReadOnlyMode,
    request: Json<Start>,
) -> AppResult<Json<ReviewRound>> {
    tokio::task::spawn_blocking(move || {
        let request = request.into_inner();
        let round = create(&request).map_err(failure)?;
        TREE.store
            .update_acceptance(&request.kind, |previous| {
                Ok(if request.new_round {
                    round
                } else {
                    previous.unwrap_or(round)
                })
            })
            .map(Json)
            .map_err(failure)
    })
    .await
    .map_err(|_| AppError::new(ErrorKind::Internal, "Could not start review"))?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    kind: String,
    round: String,
    hash: String,
    after: bool,
    judgments: Vec<Option<Judgment>>,
    #[serde(default)]
    feedback: String,
    #[serde(default)]
    effort_seconds: u32,
}

fn apply(round: &mut ReviewRound, request: &Decision) -> Result<()> {
    ensure!(
        round.id == request.round
            && request.feedback.len() <= 2000
            && request.effort_seconds <= 86400,
        "Invalid decision"
    );
    let item = round
        .photos
        .iter_mut()
        .find(|photo| photo.hash == request.hash)
        .ok_or_else(|| anyhow::anyhow!("Photo not in round"))?;
    let predictions = if request.after {
        item.after
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No after sample"))?
    } else {
        &mut item.before
    };
    ensure!(
        predictions.len() == request.judgments.len()
            && request.judgments.iter().all(Option::is_some),
        "Judge each label or mark uncertain"
    );
    for (prediction, judgment) in predictions.iter_mut().zip(&request.judgments) {
        prediction.judgment.clone_from(judgment);
    }
    if request.after {
        item.seen_after = true;
    } else {
        item.seen_before = true;
    }
    round.feedback.clone_from(&request.feedback);
    round.effort_seconds = round.effort_seconds.max(request.effort_seconds);
    Ok(())
}

#[put("/put/acceptance-review", format = "json", data = "<request>")]
pub fn decide(
    _auth: GuardAuth,
    _writable: GuardReadOnlyMode,
    request: Json<Decision>,
) -> AppResult<Json<ReviewRound>> {
    let request = request.into_inner();
    TREE.store
        .update_acceptance(&request.kind, |previous| {
            let mut round = previous.ok_or_else(|| anyhow::anyhow!("No round"))?;
            apply(&mut round, &request)?;
            Ok(round)
        })
        .map(Json)
        .map_err(failure)
}

#[put("/put/acceptance-review/after/<round_id>")]
pub fn capture_after(
    _auth: GuardAuth,
    _writable: GuardReadOnlyMode,
    round_id: &str,
) -> AppResult<Json<ReviewRound>> {
    let tags = embedding::service()
        .and_then(|service| service.auto_tags.get())
        .ok_or_else(|| AppError::new(ErrorKind::Unavailable, "Auto tags not ready"))?;
    if !tags.recalculation_status().up_to_date {
        return Err(AppError::new(
            ErrorKind::Conflict,
            "Wait for learning to complete",
        ));
    }
    TREE.store
        .update_acceptance("garden", |previous| {
            let mut round = previous.ok_or_else(|| anyhow::anyhow!("No garden round"))?;
            ensure!(
                round.id == round_id && round.after_captured_at.is_none(),
                "After sample already frozen"
            );
            ensure!(
                BuiltinCatalog::embedded()?.version == round.vocabulary
                    && embedding::service().unwrap().status().model_id.as_deref()
                        == Some(round.model.as_str()),
                "Model changed"
            );
            let concept = tag_feedback::concept_id(&round.plant);
            for item in &round.photos {
                let record = TREE
                    .store
                    .reader()?
                    .get(&item.hash)?
                    .ok_or_else(|| anyhow::anyhow!("Photo missing"))?
                    .into_value();
                let positive = record
                    .tag()
                    .iter()
                    .any(|tag| tag_feedback::concept_id(tag) == concept);
                if item.role == "teaching" {
                    ensure!(
                        positive,
                        "Teach the identified plant on every teaching photo first"
                    );
                } else {
                    ensure!(
                        !positive
                            && !TREE
                                .store
                                .auto_tag_rejections_for(&item.hash)?
                                .contains(&concept),
                        "Held-out target was taught"
                    );
                }
            }
            ensure!(
                tags.recalculation_status()
                    .last_completed_at
                    .is_some_and(|time| time > round.materialized_at),
                "Learning has not completed since baseline"
            );
            for item in &mut round.photos {
                if item.role == "heldOut" {
                    ensure!(item.seen_before, "Review baseline first");
                    item.after = Some(photo(&item.hash, "heldOut")?.before);
                }
            }
            round.after_captured_at = Some(chrono::Utc::now().timestamp_millis());
            Ok(round)
        })
        .map(Json)
        .map_err(failure)
}

#[cfg(test)]
mod tests;
