//! Generation faults run in separate processes: `DATA_PATH`, `TREE` and the fault
//! selector are process globals. All media and storage below are disposable.
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, atomic::Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use image::{DynamicImage, ImageFormat, RgbImage};

use super::artifact_publisher::ArtifactPublisher;
use super::media_pipeline::{
    MediaStage, MediaTaskPlan, ReindexOperation, ThumbnailPublishMode, backfill_small_thumbnail,
    execute_media_pipeline,
};
use super::media_publish::load_logical_media;
use crate::public::constant::storage::{DATA_PATH, get_data_path};
use crate::public::db::tree::{TREE, VERSION_COUNT_TIMESTAMP};
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::image::{ImageCombined, ImageMetadata};
use crate::public::structure::object::{ObjectSchema, ObjectType};
use crate::public::structure::video::{VideoCombined, VideoMetadata};
use crate::router::fairing::{guard_auth::GuardAuth, guard_read_only_mode::GuardReadOnlyMode};

static FAIL_DESTINATION: Mutex<Option<PathBuf>> = Mutex::new(None);
const PARTIAL: &[u8] = b"injected partial output";

/// Match the exact destination directory and version, allowing random job tokens.
fn take_fault(path: &Path) -> bool {
    let mut fault = FAIL_DESTINATION.lock().unwrap();
    if let Some(destination) = fault.as_ref()
        && path.parent() == destination.parent()
        && path.extension() == destination.extension()
        && path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!(
                "{}.",
                destination.file_stem().unwrap().to_string_lossy()
            ))
        && path.to_string_lossy().contains(".tmp.")
    {
        *fault = None;
        return true;
    }
    false
}

struct FailingWriter {
    file: fs::File,
    remaining: usize,
}

impl FailingWriter {
    fn new(path: &Path) -> Result<Self> {
        fs::create_dir_all(path.parent().unwrap())?;
        Ok(Self {
            file: fs::File::create(path)?,
            remaining: PARTIAL.len(),
        })
    }
}

impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("injected partial generation failure"));
        }
        let written = self.file.write(&bytes[..bytes.len().min(self.remaining)])?;
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Fail inside the real JPEG encoder after it has written part of the file.
pub(crate) fn jpeg_write_failure(image: &RgbImage, path: &Path) -> Result<()> {
    if take_fault(path) {
        image::codecs::jpeg::JpegEncoder::new(FailingWriter::new(path)?).encode_image(image)?;
        panic!("JPEG encoder unexpectedly fit in the fault budget");
    }
    Ok(())
}

/// Simulate partial output from an external encoder or upload copy.
pub(crate) fn partial_write_failure(path: &Path) -> Result<()> {
    if take_fault(path) {
        let mut writer = FailingWriter::new(path)?;
        writer.write_all(PARTIAL)?;
        writer.write_all(b"more output")?;
        panic!("writer unexpectedly exceeded the fault budget");
    }
    Ok(())
}

fn isolated(name: &str, test: impl FnOnce()) {
    if std::env::var("UROCISSA_STAGING_TEST").as_deref() == Ok(name) {
        DATA_PATH.set(std::env::current_dir().unwrap()).unwrap();
        test();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("process::staging_failure_tests::{name}"),
            "--nocapture",
        ])
        .env("UROCISSA_STAGING_TEST", name)
        .current_dir(root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("isolated staging test timed out: {name}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{name}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_image(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 24, image::Rgb([20, 80, 160])))
        .save_with_format(path, ImageFormat::Jpeg)
        .unwrap();
}

fn fixture(video: bool) -> AbstractData {
    let hash = arrayvec::ArrayString::from(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    let mut data = if video {
        AbstractData::Video(VideoCombined {
            object: ObjectSchema::new(hash, ObjectType::Video),
            metadata: VideoMetadata::new(hash, 100, 32, 24, "mp4".to_owned()),
        })
    } else {
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(hash, ObjectType::Image),
            metadata: ImageMetadata::new(hash, 100, 32, 24, "jpg".to_owned()),
        })
    };
    data.set_cache_version(7);
    data.set_thumbhash(vec![1, 2, 3]);
    data.tag_mut().insert("keep metadata".to_owned());
    if video {
        fs::create_dir_all(data.imported_path().parent().unwrap()).unwrap();
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=blue:s=32x24:r=2",
                    "-t",
                    "1",
                    "-threads",
                    "1",
                    "-c:v",
                    "mpeg4",
                ])
                .arg(data.imported_path())
                .status()
                .unwrap()
                .success()
        );
        fs::create_dir_all(data.compressed_path().parent().unwrap()).unwrap();
        fs::write(data.compressed_path(), b"previous compressed video").unwrap();
    } else {
        write_image(&data.imported_path());
    }
    write_image(&data.thumbnail_path_for_version(7));
    write_image(&data.small_thumbnail_path_for_version(7));
    TREE.store.write(|writer| writer.insert(&data)).unwrap();
    TREE.state
        .write()
        .unwrap()
        .apply_batch(std::slice::from_ref(&data), &HashSet::new());
    data
}

fn object_bytes() -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(get_data_path().join("object"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| (entry.path().to_path_buf(), fs::read(entry.path()).unwrap()))
        .collect()
}

fn assert_object_bytes(before: &BTreeMap<PathBuf, Vec<u8>>) {
    let after = object_bytes();
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>(),
        "no staged or next-version artifacts may remain"
    );
    for (path, bytes) in before {
        assert!(
            after[path] == *bytes,
            "published bytes changed: {}",
            path.display()
        );
    }
}

fn metadata(data: &AbstractData) -> (serde_json::Value, serde_json::Value, String, i64) {
    let (slot, logical) = load_logical_media(data.hash()).unwrap();
    let durable = TREE
        .store
        .read(|reader| reader.get(data.hash().as_str()))
        .unwrap()
        .unwrap();
    (
        serde_json::to_value(durable.into_value()).unwrap(),
        serde_json::to_value(logical).unwrap(),
        format!("{:?}", TREE.state.read().unwrap().get(slot).unwrap()),
        VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed),
    )
}

fn staged_path(destination: &Path) -> PathBuf {
    destination.with_file_name(format!(
        "{}.fault-test.tmp.{}",
        destination.file_stem().unwrap().to_string_lossy(),
        destination.extension().unwrap().to_string_lossy(),
    ))
}

fn pipeline_failure(video: bool, operation: ReindexOperation, grid: bool) {
    let data = fixture(video);
    let before = object_bytes();
    let before_metadata = metadata(&data);
    for mode in [
        ThumbnailPublishMode::Initial,
        ThumbnailPublishMode::ReplaceExisting,
    ] {
        let version = if mode == ThumbnailPublishMode::Initial {
            7
        } else {
            8
        };
        let destination = if operation == ReindexOperation::VideoCompression {
            data.compressed_path()
        } else if grid {
            data.small_thumbnail_path_for_version(version)
        } else {
            data.thumbnail_path_for_version(version)
        };
        *FAIL_DESTINATION.lock().unwrap() = Some(destination.clone());
        let mut publisher = ArtifactPublisher::new("fault-test");
        let plan = MediaTaskPlan::new(vec![operation]).unwrap();
        let error = execute_media_pipeline(&data, &plan, &mut publisher, mode).unwrap_err();
        assert_eq!(
            error.stage,
            if operation == ReindexOperation::VideoCompression {
                MediaStage::VideoCompression
            } else {
                MediaStage::Thumbnail
            }
        );
        assert!(
            error
                .to_string()
                .contains("injected partial generation failure")
        );
        assert!(
            FAIL_DESTINATION.lock().unwrap().is_none(),
            "fault must be reached"
        );
        let partial = staged_path(&destination);
        assert_eq!(
            fs::metadata(partial).unwrap().len(),
            u64::try_from(PARTIAL.len()).unwrap()
        );
        if grid {
            let viewer = staged_path(&data.thumbnail_path_for_version(version));
            assert!(
                image::open(viewer).is_ok(),
                "viewer must finish before grid fails"
            );
        }
        drop(publisher);
        assert_eq!(
            metadata(&data),
            before_metadata,
            "generation must not publish metadata"
        );
        assert_object_bytes(&before);
    }
}

#[test]
fn image_grid_failure() {
    isolated("image_grid_failure", || {
        pipeline_failure(false, ReindexOperation::Thumbnail, true);
    });
}

#[test]
fn video_grid_failure() {
    isolated("video_grid_failure", || {
        pipeline_failure(true, ReindexOperation::Thumbnail, true);
    });
}

#[test]
fn image_viewer_failure() {
    isolated("image_viewer_failure", || {
        pipeline_failure(false, ReindexOperation::Thumbnail, false);
    });
}

#[test]
fn video_viewer_failure() {
    isolated("video_viewer_failure", || {
        pipeline_failure(true, ReindexOperation::Thumbnail, false);
    });
}

#[test]
fn video_compression_failure() {
    isolated("video_compression_failure", || {
        pipeline_failure(true, ReindexOperation::VideoCompression, false);
    });
}

#[test]
fn rotation_generation_failure() {
    isolated("rotation_generation_failure", || {
        let data = fixture(false);
        let before = object_bytes();
        let before_metadata = metadata(&data);
        *FAIL_DESTINATION.lock().unwrap() = Some(data.thumbnail_path_for_version(8));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(crate::router::put::rotate_image::rotate_image(
            Ok(GuardAuth),
            Ok(GuardReadOnlyMode),
            rocket::serde::json::Json(crate::router::put::rotate_image::RotateImageRequest {
                hash: data.hash().to_string(),
            }),
        ));
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected partial generation failure")
        );
        assert!(
            FAIL_DESTINATION.lock().unwrap().is_none(),
            "fault must be reached"
        );
        assert_eq!(metadata(&data), before_metadata);
        assert_object_bytes(&before);
    });
}

#[test]
fn captured_frame_partial_copy_failure() {
    isolated("captured_frame_partial_copy_failure", || {
        captured_frame_failure(true);
    });
}

#[test]
fn captured_frame_decode_failure() {
    isolated("captured_frame_decode_failure", || {
        captured_frame_failure(false);
    });
}

fn captured_frame_failure(partial_copy: bool) {
    use crate::router::put::regenerate_thumbnail::{
        RegenerateThumbnailForm, regenerate_thumbnail_with_frame,
    };
    let data = fixture(true);
    let before = object_bytes();
    let before_metadata = metadata(&data);
    if partial_copy {
        *FAIL_DESTINATION.lock().unwrap() = Some(data.thumbnail_path_for_version(8));
    }
    let form = RegenerateThumbnailForm {
        hash: data.hash().to_string(),
        frame: rocket::fs::TempFile::Buffered {
            content: b"invalid captured JPEG",
        },
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(regenerate_thumbnail_with_frame(
        Ok(GuardAuth),
        Ok(GuardReadOnlyMode),
        Ok(rocket::form::Form::from(form)),
    ));
    let expected = if partial_copy {
        "injected partial generation failure"
    } else {
        "failed to decode captured video frame"
    };
    assert!(result.unwrap_err().to_string().contains(expected));
    assert!(
        FAIL_DESTINATION.lock().unwrap().is_none(),
        "fault must be reached"
    );
    assert_eq!(metadata(&data), before_metadata);
    assert_object_bytes(&before);
}

#[test]
fn backfill_partial_encoding_failure() {
    isolated("backfill_partial_encoding_failure", || {
        let data = fixture(false);
        let destination = data.small_thumbnail_path_for_version(7);
        fs::remove_file(&destination).unwrap();
        let before = object_bytes();
        let before_metadata = metadata(&data);
        *FAIL_DESTINATION.lock().unwrap() = Some(destination.clone());
        let result = backfill_small_thumbnail(
            &data.thumbnail_path_for_version(7),
            &destination,
            "backfill-fault",
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected partial generation failure")
        );
        assert!(FAIL_DESTINATION.lock().unwrap().is_none());
        assert_eq!(metadata(&data), before_metadata);
        assert_object_bytes(&before);
    });
}
