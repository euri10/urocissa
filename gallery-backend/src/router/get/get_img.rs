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

fn check_public_media(auth: GuardShare, path: &std::path::Path) -> AppResult<()> {
    if !matches!(auth.claims.role, crate::router::claims::Role::Public) {
        return Ok(());
    }
    let hash = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(crate::public::structure::abstract_data::parse_thumbnail_stem)
        .map(|(hash, _)| hash)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Item not found"))?;
    let durable = crate::public::db::tree::TREE
        .store
        .read(|table| {
            table
                .get(hash)
                .map(|value| value.map(crate::storage::store::RecordValue::into_value))
        })
        .map_err(|error| AppError::from_err(ErrorKind::Database, error))?;
    if !crate::public::db::write_behind::WRITE_BEHIND
        .logical_record(hash, durable)
        .is_some_and(|data| !data.is_trashed())
    {
        return Err(AppError::new(ErrorKind::NotFound, "Item not found"));
    }
    Ok(())
}

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
    check_public_media(auth_guard?, &file_path)?;
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
/// The fallback keeps this deployable ahead of a `gridThumbnail` backfill: media
/// indexed before the grid tier existed still renders, just at the old
/// transfer size, until the backfill produces the small artifact.
///
/// # Errors
///
/// Returns an error if authorization, path validation, or file opening fails.
#[get("/object/thumbnail/<file_path..>")]
pub async fn small_thumbnail_file(
    auth_guard: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHash>,
    file_path: PathBuf,
) -> AppResult<GridThumbnailResponse> {
    check_public_media(auth_guard?, &file_path)?;
    let _ = hash_guard?;
    if file_path.extension().and_then(std::ffi::OsStr::to_str) != Some("jpg") {
        return Err(
            AppError::new(ErrorKind::InvalidInput, "Grid thumbnails are JPEG only")
                .context(format!("File path: {}", file_path.display())),
        );
    }
    open_grid_thumbnail(get_data_path(), &file_path).await
}

/// A grid image with a cache policy that prevents retaining viewer fallbacks.
#[derive(Responder)]
pub struct GridThumbnailResponse {
    file: NamedFile,
    cache_control: rocket::http::Header<'static>,
}

async fn open_grid_thumbnail(
    root: &std::path::Path,
    file_path: &std::path::Path,
) -> AppResult<GridThumbnailResponse> {
    let path = resolve_grid_thumbnail(root, file_path);
    let is_grid = path == root.join("object/thumbnail").join(file_path);
    let file = NamedFile::open(&path).await.or_raise(|| {
        (
            ErrorKind::IO,
            format!("Failed to open grid thumbnail: {}", path.display()),
        )
    })?;
    Ok(GridThumbnailResponse {
        file,
        cache_control: rocket::http::Header::new(
            "Cache-Control",
            if is_grid {
                "max-age=31536000, public"
            } else {
                "no-store"
            },
        ),
    })
}

#[get("/object/imported/<file_path..>")]
pub async fn imported_file(
    auth: GuardResult<GuardShare>,
    hash_guard: GuardResult<GuardHashOriginal>,
    file_path: PathBuf,
) -> AppResult<CompressedFileResponse<'static>> {
    check_public_media(auth?, &file_path)?;
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

    #[get("/object/thumbnail/<file_path..>")]
    async fn fixture(
        root: &rocket::State<std::path::PathBuf>,
        file_path: std::path::PathBuf,
    ) -> crate::router::AppResult<super::GridThumbnailResponse> {
        super::open_grid_thumbnail(root, &file_path).await
    }

    #[rocket::async_test]
    async fn fallback_is_not_cached_but_backfilled_grid_is() {
        let root = tempfile::tempdir().unwrap();
        let relative = Path::new("ab/abc-v7.jpg");
        let viewer = root.path().join("object/compressed").join(relative);
        std::fs::create_dir_all(viewer.parent().unwrap()).unwrap();
        std::fs::write(&viewer, b"viewer JPEG").unwrap();
        let client = rocket::local::asynchronous::Client::tracked(
            rocket::build()
                .manage(root.path().to_path_buf())
                .attach(crate::router::fairing::cache_control_fairing::cache_control_fairing())
                .mount("/", routes![fixture]),
        )
        .await
        .unwrap();
        let response = client
            .get("/object/thumbnail/ab/abc-v7.jpg")
            .dispatch()
            .await;
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("no-store")
        );
        assert_eq!(response.into_bytes().await.unwrap(), b"viewer JPEG");

        let grid = root.path().join("object/thumbnail").join(relative);
        std::fs::create_dir_all(grid.parent().unwrap()).unwrap();
        std::fs::write(&grid, b"grid JPEG").unwrap();
        let response = client
            .get("/object/thumbnail/ab/abc-v7.jpg")
            .dispatch()
            .await;
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("max-age=31536000, public")
        );
        assert_eq!(response.into_bytes().await.unwrap(), b"grid JPEG");
    }

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
