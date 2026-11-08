use std::collections::BTreeSet;

use rocket::serde::{Deserialize, Serialize, json::Json};

use crate::{
    operations::{auto_tags, autotag::BuiltinCatalog, tag_feedback},
    public::{
        db::tree::{TREE, state::TargetSet},
        error::{AppError, ErrorKind},
        structure::{audience::Audience, object::ObjectType},
    },
    router::{
        AppResult, GuardResult,
        fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode},
        selection::{
            ResolvedSelection, SelectionDescriptor, resolve_selection,
            resolved_selection_is_current,
        },
    },
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    hash: Option<String>,
    timestamp: Option<i64>,
    selection: Option<SelectionDescriptor>,
}

impl Target {
    fn resolve(self) -> AppResult<ResolvedSelection> {
        let resolved = match (self.hash, self.timestamp, self.selection) {
            (Some(hash), None, None) => {
                if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(AppError::new(ErrorKind::InvalidInput, "Invalid photo hash"));
                }
                let state = TREE
                    .state
                    .read()
                    .map_err(|_| AppError::new(ErrorKind::Internal, "tree lock poisoned"))?;
                let slot = state
                    .find(&hash)
                    .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Photo not found"))?;
                ResolvedSelection {
                    targets: TargetSet::from_slot_refs([slot], state.arena.capacity()),
                    len: 1,
                    identity_epoch: state.identity_epoch(),
                    selection_epoch: state.selection_epoch(),
                }
            }
            (None, Some(timestamp), Some(selection)) => resolve_selection(timestamp, selection)?,
            _ => {
                return Err(AppError::new(
                    ErrorKind::InvalidInput,
                    "Choose a photo hash or a timestamp and selection",
                ));
            }
        };
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "tree lock poisoned"))?;
        if !resolved_selection_is_current(
            &state,
            resolved.identity_epoch,
            resolved.selection_epoch,
            &resolved.targets,
        ) {
            return Err(AppError::new(ErrorKind::Conflict, "selection is stale"));
        }
        if resolved.targets.iter().any(|s| {
            state
                .get(s)
                .is_some_and(|r| r.object_type == ObjectType::Album)
        }) {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "Auto tags cannot be applied to albums",
            ));
        }
        let targets = TargetSet::from_slot_refs(
            resolved.targets.iter().filter(|s| {
                state
                    .get(*s)
                    .is_some_and(|r| r.object_type == ObjectType::Image)
            }),
            state.arena.capacity(),
        );
        Ok(ResolvedSelection {
            len: targets.len(),
            targets,
            ..resolved
        })
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Confirm,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    #[serde(flatten)]
    target: Target,
    concept: String,
    action: Action,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPreview {
    photos: usize,
    public_photos: usize,
    concepts: Vec<ConceptChoice>,
}

#[derive(Serialize)]
pub struct ConceptChoice {
    concept: String,
    label: String,
}

#[derive(Serialize)]
pub struct DecisionResult {
    photos: usize,
    label: String,
}

#[post("/post/auto_tag_selection", format = "json", data = "<data>")]
pub async fn preview(
    auth: GuardResult<GuardAuth>,
    data: Json<Target>,
) -> AppResult<Json<SelectionPreview>> {
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let resolved = data.into_inner().resolve()?;
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "tree lock poisoned"))?;
        let audiences = TREE
            .store
            .photo_audiences()
            .map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
        let mut public_photos = 0;
        for slot in resolved.targets.iter() {
            let record = state
                .get(slot)
                .ok_or_else(|| AppError::new(ErrorKind::Conflict, "selection is stale"))?;
            if audiences
                .get(record.id.as_str())
                .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
                == Audience::Public
            {
                public_photos += 1;
            }
        }
        let catalog =
            BuiltinCatalog::embedded().map_err(|e| AppError::from_err(ErrorKind::Internal, e))?;
        Ok(Json(SelectionPreview {
            photos: resolved.len,
            public_photos,
            concepts: catalog
                .concepts
                .into_iter()
                .map(|c| ConceptChoice {
                    concept: c.id,
                    label: c.label_fr,
                })
                .collect(),
        }))
    })
    .await
    .map_err(|e| AppError::from_err(ErrorKind::Internal, e.into()))?
}

#[put("/put/auto_tag_decision", format = "json", data = "<data>")]
pub async fn decide(
    auth: GuardResult<GuardAuth>,
    writable: GuardResult<GuardReadOnlyMode>,
    data: Json<Decision>,
) -> AppResult<Json<DecisionResult>> {
    let _ = auth?;
    let _ = writable?;
    tokio::task::spawn_blocking(move || {
        let data = data.into_inner();
        let catalog =
            BuiltinCatalog::embedded().map_err(|e| AppError::from_err(ErrorKind::Internal, e))?;
        let concept = catalog
            .concepts
            .iter()
            .find(|c| c.id == data.concept)
            .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Unknown Concept"))?;
        let resolved = data.target.resolve()?;
        let add = match data.action {
            Action::Confirm => BTreeSet::from([concept.label_fr.clone()]),
            Action::Reject => BTreeSet::new(),
        };
        let rejection = matches!(data.action, Action::Reject).then_some(concept.id.as_str());
        let count = tag_feedback::edit(&resolved, &add, &BTreeSet::new(), rejection)?;
        auto_tags::rematerialize();
        Ok(Json(DecisionResult {
            photos: count,
            label: concept.label_fr.clone(),
        }))
    })
    .await
    .map_err(|e| AppError::from_err(ErrorKind::Internal, e.into()))?
}
