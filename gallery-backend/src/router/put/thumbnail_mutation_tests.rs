//! Exercise the real routes in subprocesses: storage/config globals never touch
//! installed data or other tests, and Rocket's local client opens no port.
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arrayvec::ArrayString;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use rocket::http::{ContentType, Cookie, Header, Status};
use rocket::local::asynchronous::Client;

use crate::operations::indexation::generate_thumbnail::generate_small_thumbnail_from;
use crate::process::media_publish::load_logical_media;
use crate::public::constant::storage::DATA_PATH;
use crate::public::db::tree::TREE;
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use crate::public::structure::image::{ImageCombined, ImageMetadata};
use crate::public::structure::object::{ObjectSchema, ObjectType};
use crate::public::structure::video::{VideoCombined, VideoMetadata};
use crate::router::claims::claims::Claims;
use crate::router::claims::claims_hash::ClaimsHash;

const HASH: &str = "3540000000000000000000000000000000000000000000000000000000000000";

static FAIL_GRID_PROMOTION: AtomicBool = AtomicBool::new(false);

// Called only in test builds, after both encoders have finished. Removing the
// second staged file exercises rollback after the viewer has been promoted.
pub(super) fn before_publish(staged_grid: &Path) {
    if FAIL_GRID_PROMOTION.load(Ordering::Relaxed) {
        std::fs::remove_file(staged_grid).unwrap();
    }
}

fn isolated(name: &str) -> bool {
    if std::env::var("UROCISSA_THUMBNAIL_TEST").as_deref() == Ok(name) {
        DATA_PATH.set(std::env::current_dir().unwrap()).unwrap();
        return false;
    }
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{}::{name}", module_path!().split_once("::").unwrap().1),
            "--nocapture",
        ])
        .env("UROCISSA_THUMBNAIL_TEST", name)
        .current_dir(root.path())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "isolated {name} failed: {status}");
            return true;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("isolated {name} exceeded 90 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pattern() -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(960, 640, |x, y| {
        match (x < 480, y < 320) {
            (true, true) => Rgb([230, 20, 20]),
            (false, true) => Rgb([20, 230, 20]),
            (true, false) => Rgb([20, 20, 230]),
            (false, false) => Rgb([230, 230, 20]),
        }
    }))
}

fn seed(video: bool, previous_grid: bool) -> AbstractData {
    let id = ArrayString::from(HASH).unwrap();
    let mut data = if video {
        AbstractData::Video(VideoCombined {
            object: ObjectSchema::new(id, ObjectType::Video),
            metadata: VideoMetadata::new(id, 0, 96, 64, "mp4".into()),
        })
    } else {
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 0, 960, 640, "png".into()),
        })
    };
    data.set_cache_version(7);
    let original = data.imported_path();
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    if video {
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=purple:s=96x64:r=1",
                    "-frames:v",
                    "1",
                    "-threads",
                    "1"
                ])
                .arg(&original)
                .stdin(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir_all(data.compressed_path_parent()).unwrap();
        std::fs::copy(&original, data.compressed_path()).unwrap();
    } else {
        pattern()
            .save_with_format(&original, ImageFormat::Png)
            .unwrap();
    }
    let viewer = data.thumbnail_path_for_version(7);
    std::fs::create_dir_all(viewer.parent().unwrap()).unwrap();
    pattern()
        .save_with_format(&viewer, ImageFormat::Jpeg)
        .unwrap();
    if previous_grid {
        generate_small_thumbnail_from(&viewer, &data.small_thumbnail_path_for_version(7)).unwrap();
    }
    TREE.store.write(|writer| writer.insert(&data)).unwrap();
    TREE.state
        .write()
        .unwrap()
        .apply_batch(std::slice::from_ref(&data), &HashSet::new());
    data
}

async fn client() -> Client {
    let mut config = AppConfig::default();
    config.private.auth_key = Some("dummy-thumbnail-mutation-tests-only".into());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    let client = Client::tracked(
        rocket::custom(rocket::Config::figment().merge(("log_level", "off")))
            .attach(crate::router::fairing::cache_control_fairing::cache_control_fairing())
            .mount(
                "/",
                routes![
                    super::rotate_image::rotate_image,
                    super::regenerate_thumbnail::regenerate_thumbnail_with_frame,
                    crate::router::get::get_img::small_thumbnail_file,
                ],
            ),
    )
    .await
    .unwrap();
    client
        .cookies()
        .add(Cookie::new("jwt", Claims::new_admin().encode()));
    client
}

async fn mutate(client: &Client, video: bool) -> Status {
    if video {
        let mut frame = Cursor::new(Vec::new());
        pattern().write_to(&mut frame, ImageFormat::Png).unwrap();
        let mut body = format!("--frame-boundary\r\nContent-Disposition: form-data; name=\"hash\"\r\n\r\n{HASH}\r\n--frame-boundary\r\nContent-Disposition: form-data; name=\"frame\"; filename=\"frame.png\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
        body.extend(frame.into_inner());
        body.extend(b"\r\n--frame-boundary--\r\n");
        client
            .put("/put/regenerate-thumbnail-with-frame")
            .header(
                ContentType::parse_flexible("multipart/form-data; boundary=frame-boundary")
                    .unwrap(),
            )
            .body(body)
            .dispatch()
            .await
            .status()
    } else {
        client
            .put("/put/rotate-image")
            .header(ContentType::JSON)
            .body(format!("{{\"hash\":\"{HASH}\"}}"))
            .dispatch()
            .await
            .status()
    }
}

fn assert_pixels(path: &Path, expected: &DynamicImage, dimensions: (u32, u32)) {
    let actual = image::open(path).unwrap().to_rgb8();
    assert_eq!(actual.dimensions(), dimensions);
    let expected = expected.to_rgb8();
    for (x, y) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
        let actual = actual.get_pixel(actual.width() * x / 4, actual.height() * y / 4);
        let expected = expected.get_pixel(expected.width() * x / 4, expected.height() * y / 4);
        for channel in 0..3 {
            assert!(
                actual[channel].abs_diff(expected[channel]) < 15,
                "wrong orientation/frame: {actual:?} != {expected:?}"
            );
        }
    }
}

async fn success_case(video: bool, previous_grid: bool) {
    let data = seed(video, previous_grid);
    let original = std::fs::read(data.imported_path()).unwrap();
    let compressed_video = video.then(|| std::fs::read(data.compressed_path()).unwrap());
    let expected = if video {
        pattern()
    } else {
        image::open(data.thumbnail_path()).unwrap().rotate270()
    };
    let client = client().await;
    assert_eq!(mutate(&client, video).await, Status::Ok);
    let (_, latest) = load_logical_media(data.hash()).unwrap();
    assert_eq!(latest.cache_version(), 8);
    assert_eq!(
        (latest.width(), latest.height()),
        if video { (96, 64) } else { (640, 960) }
    );
    assert_pixels(
        &latest.thumbnail_path_for_version(8),
        &expected,
        if video { (960, 640) } else { (640, 960) },
    );
    let grid = latest.small_thumbnail_path_for_version(8);
    assert!(grid.is_file(), "mutation must publish the next grid tier");
    assert_pixels(
        &grid,
        &expected,
        if video { (768, 512) } else { (512, 768) },
    );
    assert!(!data.thumbnail_path_for_version(7).exists());
    assert!(!data.small_thumbnail_path_for_version(7).exists());
    assert_eq!(std::fs::read(data.imported_path()).unwrap(), original);
    if let Some(bytes) = compressed_video {
        assert_eq!(std::fs::read(data.compressed_path()).unwrap(), bytes);
    }
    let response = client
        .get(format!("/object/thumbnail/35/{HASH}-v8.jpg"))
        .header(Header::new(
            "Authorization",
            format!("Bearer {}", ClaimsHash::new(data.hash(), 0, false).encode()),
        ))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.headers().get_one("Cache-Control"),
        Some("max-age=31536000, public")
    );
    assert_eq!(
        response.into_bytes().await.unwrap(),
        std::fs::read(grid).unwrap()
    );
    assert_no_staging();
}

fn assert_no_staging() {
    for entry in walkdir::WalkDir::new(DATA_PATH.get().unwrap().join("object")) {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy();
        assert!(
            !name.contains(".tmp.") && !name.contains(".bak."),
            "leftover artifact: {}",
            entry.path().display()
        );
    }
}

#[test]
fn rotation_publishes_and_retires_both_tiers() {
    if isolated("rotation_publishes_and_retires_both_tiers") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(success_case(false, true));
}

#[test]
fn captured_frame_publishes_and_retires_both_tiers() {
    if isolated("captured_frame_publishes_and_retires_both_tiers") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(success_case(true, true));
}

#[test]
fn rotation_without_previous_grid() {
    if isolated("rotation_without_previous_grid") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(success_case(false, false));
}

#[test]
fn captured_frame_without_previous_grid() {
    if isolated("captured_frame_without_previous_grid") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(success_case(true, false));
}

#[cfg(unix)]
async fn failure_case(video: bool) {
    use std::os::unix::fs::PermissionsExt;

    let data = seed(video, true);
    let client = client().await;
    let paths = [
        data.imported_path(),
        data.thumbnail_path_for_version(7),
        data.small_thumbnail_path_for_version(7),
    ];
    let bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    let metadata = serde_json::to_value(&data).unwrap();
    let grid_parent = paths[2].parent().unwrap();
    let permissions = std::fs::metadata(grid_parent).unwrap().permissions();

    for failure in ["generation", "promotion", "commit"] {
        match failure {
            // A real grid write error, after viewer generation. Existing grid
            // stays readable. No permission changes escape this subprocess.
            "generation" => {
                std::fs::set_permissions(grid_parent, std::fs::Permissions::from_mode(0o555))
                    .unwrap();
            }
            "promotion" => FAIL_GRID_PROMOTION.store(true, Ordering::Relaxed),
            "commit" => {
                // The real durable-commit path fails acquiring this lock after
                // promotion; the publisher must restore both tiers.
                let _ = std::thread::spawn(|| {
                    let _guard = TREE.persistence_lock.lock().unwrap();
                    panic!("injected metadata-commit failure");
                })
                .join();
            }
            _ => unreachable!(),
        }
        let status = mutate(&client, video).await;
        std::fs::set_permissions(grid_parent, permissions.clone()).unwrap();
        FAIL_GRID_PROMOTION.store(false, Ordering::Relaxed);
        assert_eq!(status, Status::InternalServerError, "{failure} must fail");
        let (_, latest) = load_logical_media(data.hash()).unwrap();
        assert_eq!(
            serde_json::to_value(latest).unwrap(),
            metadata,
            "{failure} changed logical metadata"
        );
        let durable = TREE
            .store
            .read(|reader| Ok::<_, anyhow::Error>(reader.get(HASH)?.unwrap().into_value()))
            .unwrap();
        assert_eq!(
            serde_json::to_value(durable).unwrap(),
            metadata,
            "{failure} changed durable metadata"
        );
        for (path, expected) in paths.iter().zip(&bytes) {
            assert_eq!(
                &std::fs::read(path).unwrap(),
                expected,
                "{failure} changed {}",
                path.display()
            );
        }
        assert!(
            !data.thumbnail_path_for_version(8).exists(),
            "{failure} left next viewer"
        );
        assert!(
            !data.small_thumbnail_path_for_version(8).exists(),
            "{failure} left next grid"
        );
        assert_no_staging();
    }
}

#[cfg(unix)]
#[test]
fn rotation_failures_preserve_both_tiers_and_metadata() {
    if isolated("rotation_failures_preserve_both_tiers_and_metadata") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(failure_case(false));
}

#[cfg(unix)]
#[test]
fn captured_frame_failures_preserve_both_tiers_and_metadata() {
    if isolated("captured_frame_failures_preserve_both_tiers_and_metadata") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(failure_case(true));
}

async fn concurrent_case(video: bool) {
    let data = seed(video, true);
    let original = std::fs::read(data.imported_path()).unwrap();
    let client = client().await;
    let guard = crate::process::media_lock::lock_media(data.hash()).await;
    let first = mutate(&client, video);
    let second = mutate(&client, video);
    tokio::pin!(first, second);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut first)
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut second)
            .await
            .is_err()
    );
    assert_eq!(
        load_logical_media(data.hash()).unwrap().1.cache_version(),
        7
    );
    drop(guard);
    let (first, second) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(first, second)
    })
    .await
    .unwrap();
    assert_eq!((first, second), (Status::Ok, Status::Ok));
    let latest = load_logical_media(data.hash()).unwrap().1;
    assert_eq!(latest.cache_version(), 9);
    let expected = if video {
        pattern()
    } else {
        pattern().rotate180()
    };
    assert_pixels(&latest.thumbnail_path_for_version(9), &expected, (960, 640));
    assert_pixels(
        &latest.small_thumbnail_path_for_version(9),
        &expected,
        (768, 512),
    );
    for version in [7, 8] {
        assert!(!data.thumbnail_path_for_version(version).exists());
        assert!(!data.small_thumbnail_path_for_version(version).exists());
    }
    assert_eq!(std::fs::read(data.imported_path()).unwrap(), original);
    assert_no_staging();
}

#[test]
fn rotations_serialize_on_media_lock() {
    if isolated("rotations_serialize_on_media_lock") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(concurrent_case(false));
}

#[test]
fn captured_frames_serialize_on_media_lock() {
    if isolated("captured_frames_serialize_on_media_lock") {
        return;
    }
    rocket::tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(concurrent_case(true));
}
