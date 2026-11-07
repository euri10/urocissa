use crate::public::structure::album::ResolvedShare;

pub mod album_visibility;
pub mod autotag;
pub mod embedding;
pub mod hash;
pub mod indexation;
pub mod initialization;
pub mod open_db;
pub mod public_metadata;
pub mod transitor;
pub mod utils;
pub mod video_metadata;
pub mod visibility;

pub fn resolve_show_download_and_metadata(
    resolved_share_opt: Option<ResolvedShare>,
) -> (bool, bool) {
    resolved_share_opt.map_or((true, true), |resolved_share| {
        (
            resolved_share.share.show_download,
            resolved_share.share.show_metadata,
        )
    })
}
