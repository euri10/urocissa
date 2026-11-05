use crate::operations::visibility::{PhotoVisibility, viewer};
use crate::public::db::tree::TREE;
use crate::public::db::tree::read_tags::SearchFacets;
use crate::public::error::AppError; // Import AppError
use crate::public::structure::album::Share;
use crate::public::structure::{
    abstract_data::AbstractData,
    audience::{Audience, Viewer},
};
use crate::router::fairing::guard_gallery::GuardGallery;
use crate::router::{AppResult, GuardResult};
use arrayvec::ArrayString;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

#[get("/get/get-search-facets?<trashed>")]
pub async fn get_search_facets(
    auth: GuardResult<GuardGallery>,
    trashed: Option<bool>,
) -> AppResult<Json<SearchFacets>> {
    let trashed = if auth?.claims.is_admin() {
        trashed
    } else {
        Some(false)
    };
    tokio::task::spawn_blocking(move || -> AppResult<Json<SearchFacets>> {
        let start_time = Instant::now();
        let facets = match trashed {
            Some(trashed) => TREE.read_search_facets_for_trash_state(trashed),
            None => TREE.read_search_facets(),
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
}

#[get("/get/get-albums")]
pub async fn get_albums(auth: GuardResult<GuardGallery>) -> AppResult<Json<Vec<AlbumInfo>>> {
    let request_viewer = viewer(&auth?.claims.role);
    tokio::task::spawn_blocking(move || -> AppResult<Json<Vec<AlbumInfo>>> {
        let visibility = PhotoVisibility::new(request_viewer)?;
        let is_admin = request_viewer == Viewer::Owner;
        let start_time = Instant::now();
        let album_list = TREE.read_albums();
        let mut album_info_list = Vec::new();
        for album in album_list {
            if !visibility.allows_data(&AbstractData::Album(album.clone()))? {
                continue;
            }
            album_info_list.push(AlbumInfo {
                audience: visibility.audience(album.object.id.as_str())?,
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
