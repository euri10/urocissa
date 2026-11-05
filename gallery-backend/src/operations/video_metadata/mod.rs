//! Owner maintenance of existing canonical compressed MP4s. Originals and
//! database metadata are never opened or mutated by this operation.
mod probe;

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use arrayvec::ArrayString;
use serde::Serialize;
use tokio::process::Command;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::operations::indexation::video_metadata_policy::{
    STRIP_SOURCE_METADATA_ARGS, tags_are_clean,
};
use crate::process::media_lock::lock_media;
use crate::public::structure::abstract_data::parse_thumbnail_stem;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationVerification {
    pub total: usize,
    pub checked: usize,
    pub location_tagged: usize,
    pub failed: usize,
    pub passed: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripReport {
    pub total: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub verification: LocationVerification,
}

impl StripReport {
    fn error(&mut self, id: &str, error: &anyhow::Error) {
        if self.errors.len() < 20 {
            self.errors.push(format!("{id}: {error:#}"));
        }
    }
}

fn compressed_videos(root: &Path) -> Result<Vec<(ArrayString<64>, PathBuf)>> {
    let compressed = root.join("object/compressed");
    // Never follow a link into imported originals or another storage directory.
    match fs::symlink_metadata(root.join("object")) {
        Ok(metadata) => ensure!(
            metadata.is_dir(),
            "object directory is not a regular directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    match fs::symlink_metadata(&compressed) {
        Ok(metadata) => ensure!(
            metadata.is_dir(),
            "compressed directory is not a regular directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut videos = Vec::new();
    for entry in WalkDir::new(&compressed).min_depth(1).follow_links(false) {
        let entry = entry.context("could not enumerate compressed videos")?;
        let path = entry.path();
        ensure!(
            !entry.file_type().is_symlink() || path.is_file(),
            "compressed directory contains a directory or dangling symlink"
        );
        if path.extension().is_none_or(|ext| ext != "mp4") {
            ensure!(
                !entry.file_type().is_symlink(),
                "compressed directory contains a symlink"
            );
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !stem.is_ascii() {
            continue;
        }
        // Match the media route's filename policy, including nested/versioned
        // copies. Staged artifacts cannot be served and are excluded on retry.
        let Some((hash, _)) = parse_thumbnail_stem(stem) else {
            continue;
        };
        videos.push((
            ArrayString::from(hash).expect("validated 64-byte hash"),
            path.to_path_buf(),
        ));
    }
    videos.sort_by_key(|video| video.0);
    Ok(videos)
}

struct StagedFile(PathBuf);

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

async fn strip_one(path: &Path) -> Result<bool> {
    let before = probe::read(path)
        .await
        .context("source verification failed")?;
    if tags_are_clean(&before) && !probe::has_location(&before) {
        return Ok(false);
    }
    let hashes = probe::stream_hashes(path).await?;
    let staged_path = path.with_file_name(format!(
        ".{}.{}.metadata.tmp.mp4",
        path.file_stem()
            .context("missing compressed video name")?
            .to_string_lossy(),
        Uuid::new_v4()
    ));
    // Own cleanup only after reservation succeeds; never unlink a preexisting
    // path if create_new refuses it. No await occurs between reservation/guard.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged_path)?;
    let staged = StagedFile(staged_path);
    let mut command = Command::new("ffmpeg");
    command
        .args(["-v", "error", "-nostdin", "-xerror", "-y", "-copyts", "-i"])
        .arg(path)
        .args(["-map", "0", "-c", "copy"])
        .args(STRIP_SOURCE_METADATA_ARGS)
        .args(["-movflags", "+faststart"])
        .arg(&staged.0);
    probe::run(&mut command)
        .await
        .context("stream copy failed")?;
    let after = probe::read(&staged.0)
        .await
        .context("output verification failed")?;
    ensure!(
        tags_are_clean(&after) && !probe::has_location(&after),
        "source metadata survived stream copy"
    );
    probe::verify_playback(&before, &after)?;
    ensure!(
        hashes == probe::stream_hashes(&staged.0).await?,
        "stream payload changed"
    );
    let output = OpenOptions::new().write(true).open(&staged.0)?;
    fs::set_permissions(&staged.0, fs::metadata(path)?.permissions())?;
    output.sync_all()?;
    drop(output);
    #[cfg(test)]
    tests::before_replace(path, &staged.0).await;
    // One replace, with no backup/unlink gap. A failed rename preserves the old
    // file; interruption leaves either that file or the verified replacement.
    fs::rename(&staged.0, path).context("atomic replacement failed")?;
    Ok(true)
}

pub async fn strip_compressed_videos(root: &Path) -> Result<StripReport> {
    let videos = compressed_videos(root)?;
    let mut report = StripReport {
        total: videos.len(),
        ..StripReport::default()
    };
    for (id, path) in videos {
        let _guard = lock_media(id).await;
        match strip_one(&path).await {
            Ok(true) => report.changed += 1,
            Ok(false) => report.unchanged += 1,
            Err(error) => {
                report.failed += 1;
                report.error(id.as_str(), &error);
            }
        }
    }
    // A fresh enumeration also checks derivatives added during this job. A
    // failed traversal/probe must never be reported as a clean privacy audit.
    match compressed_videos(root) {
        Ok(videos) => {
            report.verification.total = videos.len();
            for (id, path) in videos {
                let _guard = lock_media(id).await;
                match probe::read(&path).await {
                    Ok(probe) => {
                        report.verification.checked += 1;
                        if probe::has_location(&probe) {
                            report.verification.location_tagged += 1;
                        }
                    }
                    Err(error) => {
                        report.verification.failed += 1;
                        report.error(id.as_str(), &error.context("post-run verification failed"));
                    }
                }
            }
        }
        Err(error) => {
            report.verification.failed += 1;
            report.error("scan", &error);
        }
    }
    report.verification.passed =
        report.verification.failed == 0 && report.verification.location_tagged == 0;
    Ok(report)
}

#[cfg(test)]
mod tests;
