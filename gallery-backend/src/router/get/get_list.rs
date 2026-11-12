use crate::operations::visibility::{PhotoVisibility, viewer};
use crate::public::db::tree::TREE;
use crate::public::db::tree::read_tags::SearchFacets;
use crate::public::error::{AppError, ErrorKind};
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::album::Share;
use crate::public::structure::audience::Audience;
use crate::public::structure::object::ObjectType;
use crate::router::fairing::guard_share::GuardShare;
use crate::router::{AppResult, GuardResult};
use arrayvec::ArrayString;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

#[get("/get/get-search-facets?<trashed>")]
pub async fn get_search_facets(
    auth: GuardResult<GuardShare>,
    trashed: Option<bool>,
) -> AppResult<Json<SearchFacets>> {
    let claims = auth?.claims;
    let request_viewer = viewer(&claims.role);
    let trashed = if claims.is_admin() {
        trashed
    } else {
        Some(false)
    };
    tokio::task::spawn_blocking(move || -> AppResult<Json<SearchFacets>> {
        let start_time = Instant::now();
        let visibility = PhotoVisibility::new(request_viewer)?;
        let facets = if claims
            .get_share()
            .is_some_and(|share| !share.share.show_metadata)
        {
            SearchFacets {
                tags: vec![],
                makes: vec![],
                models: vec![],
            }
        } else {
            TREE.read_search_facets(&visibility, trashed)?
        };
        crate::perf_timing!(
            "get_list.read_search_facets_response",
            start_time,
            "Create search facets response"
        );
        Ok(Json(facets))
    })
    .await
    .map_err(|e| AppError::from(anyhow::Error::from(e)))? // Handle JoinError
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub album_id: String,
    pub album_name: Option<String>,
    pub share_list: HashMap<ArrayString<64>, Share>,
    pub audience: Audience,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_private_photos: Option<bool>,
}

#[get("/get/get-albums")]
pub async fn get_albums(auth: GuardResult<GuardShare>) -> AppResult<Json<Vec<AlbumInfo>>> {
    let claims = auth?.claims;
    let is_admin = claims.is_admin();
    let request_viewer = viewer(&claims.role);
    tokio::task::spawn_blocking(move || -> AppResult<Json<Vec<AlbumInfo>>> {
        let start_time = Instant::now();
        let visibility = PhotoVisibility::new(request_viewer)?;
        let album_list = TREE.read_albums();
        let state = TREE
            .state
            .read()
            .map_err(|_| AppError::new(ErrorKind::Internal, "Tree state lock poisoned"))?;
        let mut album_info_list = Vec::new();
        for album in album_list {
            if album.object.is_trashed
                || !visibility.allows_data(&AbstractData::Album(album.clone()))?
            {
                continue;
            }
            let mut has_private_photos = false;
            if is_admin {
                for ordinal in state
                    .query
                    .albums
                    .get(&album.object.id)
                    .into_iter()
                    .flat_map(|members| members.iter())
                {
                    if !state.query.trashed.contains(ordinal)
                        && let Some(record) = state
                            .slot_for_ordinal(ordinal)
                            .and_then(|slot| state.get(slot))
                        && record.object_type != ObjectType::Album
                        && visibility.audience(record.id.as_str())? == Audience::Private
                    {
                        has_private_photos = true;
                        break;
                    }
                }
            }
            album_info_list.push(AlbumInfo {
                audience: visibility.audience(album.object.id.as_str())?,
                has_private_photos: is_admin.then_some(has_private_photos),
                album_id: album.object.id.to_string(),
                album_name: album.metadata.title,
                share_list: if is_admin {
                    album.metadata.share_list
                } else {
                    HashMap::new()
                },
            });
        }
        crate::perf_timing!(
            "get_list.read_albums_response",
            start_time,
            "Create albums response: {} items",
            album_info_list.len()
        );
        Ok(Json(album_info_list))
    })
    .await
    .map_err(|e| AppError::from(anyhow::Error::from(e)))?
}
