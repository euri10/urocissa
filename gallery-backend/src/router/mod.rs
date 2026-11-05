pub mod builder;
pub mod claims;
pub mod delete;
pub mod fairing;
pub mod get;
pub mod performance;
pub mod post;
pub mod put;
pub mod saved_searches;
pub mod selection;
pub mod strip_video_metadata;
pub use crate::public::error::{AppError, ErrorKind};
pub type AppResult<T> = Result<T, AppError>;
pub type GuardResult<T> = Result<T, AppError>;
pub type GuardError = AppError;
pub use builder::build_rocket;

#[cfg(test)]
mod anonymous_route_tests;
#[cfg(test)]
mod public_metadata_http_tests;
#[cfg(test)]
mod share_edit_http_tests;
#[cfg(test)]
mod share_marker_http_tests;

#[cfg(test)]
mod album_audience_http_tests;
#[cfg(test)]
mod audience_http_tests;
#[cfg(test)]
mod strip_video_metadata_http_tests;
