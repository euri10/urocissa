use crate::public::constant::storage::get_data_path;
use crate::public::error::{AppError, ErrorKind, ResultExt};
use crate::router::{
    AppResult, GuardResult,
    fairing::{
        guard_hash::{GuardHash, GuardHashOriginal},
        guard_share::GuardShare,
    },
};
use rocket::fs::NamedFile;
use rocket::response::Responder;
use rocket_seek_stream::SeekStream;
use std::path::PathBuf;

#[derive(Responder)]
pub enum CompressedFileResponse<'a> {
    SeekStream(SeekStream<'a>),
    NamedFile(NamedFile),
}

#[get("/object/compressed/<file_path..>")]
pub async fn compressed_file(
    auth_guard: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHash>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    let _ = auth_guard?;
    let _ = hash_guard?;
    let root = get_data_path();
    let compressed_file_path = root.join("object/compressed").join(&file_path);

    let result = match compressed_file_path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("mp4") => SeekStream::from_path(&compressed_file_path)
            .map(CompressedFileResponse::SeekStream)
            .or_raise(|| {
                (
                    ErrorKind::IO,
                    format!(
                        "Failed to open MP4 file: {}",
                        compressed_file_path.display()
                    ),
                )
            })?,
        Some("jpg") => {
            let named_file = NamedFile::open(&compressed_file_path).await.or_raise(|| {
                (
                    ErrorKind::IO,
                    format!(
                        "Failed to open JPG file: {}",
                        compressed_file_path.display()
                    ),
                )
            })?;
            CompressedFileResponse::NamedFile(named_file)
        }
        Some(ext) => {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                format!("Unsupported file extension: {ext}"),
            )
            .context(format!("File path: {}", compressed_file_path.display())));
        }
        None => {
            return Err(
                AppError::new(ErrorKind::InvalidInput, "File has no extension")
                    .context(format!("File path: {}", compressed_file_path.display())),
            );
        }
    };

    Ok(result)
}

/// Pick the grid artifact when it exists, otherwise the viewer artifact.
fn resolve_grid_thumbnail(root: &std::path::Path, file_path: &std::path::Path) -> PathBuf {
    let small_path = root.join("object/thumbnail").join(file_path);
    if small_path.is_file() {
        small_path
    } else {
        root.join("object/compressed").join(file_path)
    }
}

/// Serve the grid tier, falling back to the viewer tier when it is absent.
///
/// The fallback keeps this deployable ahead of a `Thumbnail` reindex: media
/// indexed before the grid tier existed still renders, just at the old
/// transfer size, until the backfill produces the small artifact.
#[get("/object/thumbnail/<file_path..>")]
pub async fn small_thumbnail_file(
    auth_guard: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHash>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    check_public_media(auth_guard?, &file_path)?;
    let _ = hash_guard?;
    if file_path.extension().and_then(std::ffi::OsStr::to_str) != Some("jpg") {
        return Err(
            AppError::new(ErrorKind::InvalidInput, "Grid thumbnails are JPEG only")
                .context(format!("File path: {}", file_path.display())),
        );
    }
    let path = resolve_grid_thumbnail(get_data_path().as_path(), &file_path);
    NamedFile::open(&path)
        .await
        .map(CompressedFileResponse::NamedFile)
        .or_raise(|| {
            (
                ErrorKind::IO,
                format!("Failed to open grid thumbnail: {}", path.display()),
            )
        })
}

#[get("/object/imported/<file_path..>")]
pub async fn imported_file(
    auth: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHashOriginal>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    let _ = auth?;
    let _ = hash_guard?;
    let root = get_data_path();
    let imported_file_path = root.join("object/imported").join(&file_path);
    NamedFile::open(&imported_file_path)
        .await
        .map(CompressedFileResponse::NamedFile)
        .or_raise(|| {
            (
                ErrorKind::IO,
                format!(
                    "Error opening imported file: {}",
                    imported_file_path.display()
                ),
            )
        })
}

#[cfg(test)]
mod grid_thumbnail_tests {
    use super::resolve_grid_thumbnail;
    use std::path::Path;

    #[test]
    fn prefers_the_grid_artifact_when_it_exists() {
        let root = tempfile::tempdir().unwrap();
        let relative = Path::new("ab/abc.jpg");
        let small = root.path().join("object/thumbnail").join(relative);
        std::fs::create_dir_all(small.parent().unwrap()).unwrap();
        std::fs::write(&small, b"small").unwrap();

        assert_eq!(resolve_grid_thumbnail(root.path(), relative), small);
    }

    #[test]
    fn falls_back_to_the_viewer_artifact_before_backfill() {
        let root = tempfile::tempdir().unwrap();
        let relative = Path::new("ab/abc.jpg");
        let viewer = root.path().join("object/compressed").join(relative);
        std::fs::create_dir_all(viewer.parent().unwrap()).unwrap();
        std::fs::write(&viewer, b"viewer").unwrap();

        assert_eq!(resolve_grid_thumbnail(root.path(), relative), viewer);
    }

    #[test]
    fn a_grid_directory_entry_does_not_shadow_the_viewer_artifact() {
        let root = tempfile::tempdir().unwrap();
        let relative = Path::new("ab/abc.jpg");
        std::fs::create_dir_all(root.path().join("object/thumbnail").join(relative)).unwrap();

        assert_eq!(
            resolve_grid_thumbnail(root.path(), relative),
            root.path().join("object/compressed").join(relative)
        );
    }
}
