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
