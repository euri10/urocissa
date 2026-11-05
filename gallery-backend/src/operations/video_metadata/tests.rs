//! Disposable media only: no installed data path, database, or production configuration.
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;
use std::{collections::BTreeMap, fs};

use serde_json::Value;
use tokio::process::Command;

use super::strip_compressed_videos;

const GLOBAL_TAGS: &[(&str, &str)] = &[
    ("location", "+48.8566+002.3522/"),
    ("com.apple.quicktime.location.ISO6709", "+48.8566+002.3522/"),
    ("creation_time", "2024-01-02T03:04:05.000000Z"),
    ("title", "private fixture title"),
    ("comment", "/private/fixture/video.mov"),
    ("private_custom", "unknown private fixture metadata"),
];
const PAUSE_REPLACE: &str = ".strip-pause-before-replace";
const REACHED_REPLACE: &str = ".strip-reached-before-replace";
const FAIL_REPLACE: &str = ".strip-fail-before-replace";
const CHILD_MARKER: &str = ".strip-interruption-child";

/// Faults are scoped to a disposable fixture's exact directory, including when
/// tests run concurrently. Production builds do not contain this hook.
pub(super) async fn before_replace(destination: &Path, staged: &Path) {
    let directory = destination.parent().unwrap();
    if directory.join(FAIL_REPLACE).is_file() {
        fs::remove_file(staged).unwrap();
    } else if directory.join(PAUSE_REPLACE).is_file() {
        fs::write(
            directory.join(REACHED_REPLACE),
            staged.file_name().unwrap().to_str().unwrap(),
        )
        .unwrap();
        std::future::pending::<()>().await;
    }
}

async fn run(command: &mut Command) -> Output {
    let output = tokio::time::timeout(
        Duration::from_secs(20),
        command.stdin(Stdio::null()).kill_on_drop(true).output(),
    )
    .await
    .expect("fixture media command exceeded 20 seconds")
    .unwrap();
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

async fn probe(path: &Path) -> Value {
    let output = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path))
    .await;
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn packets(path: &Path) -> Value {
    let output = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            "packet=stream_index,pts,dts,duration,data_hash,size,flags:packet_side_data",
            "-of",
            "json",
        ])
        .arg(path))
    .await;
    let probe: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut streams = BTreeMap::<u64, Vec<Value>>::new();
    for packet in probe["packets"].as_array().unwrap() {
        streams
            .entry(packet["stream_index"].as_u64().unwrap())
            .or_default()
            .push(packet.clone());
    }
    // Remuxing may interleave streams differently; each stream's packet order,
    // exact timestamps, payload and skip/discard-sample data must stay intact.
    serde_json::to_value(streams).unwrap()
}

async fn decoded_frames(path: &Path) -> Vec<u8> {
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-xerror", "-i"])
        .arg(path)
        .args(["-map", "0", "-f", "framemd5", "-"]))
    .await
    .stdout
}

fn compressed_path(root: &Path, id: u8) -> PathBuf {
    let prefix = format!("{id:02x}");
    root.join("object/compressed")
        .join(&prefix)
        .join(format!("{}.mp4", prefix.repeat(32)))
}

fn copy(source: &Path, destination: &Path) {
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(source, destination).unwrap();
}

async fn tagged_fixture(path: &Path) {
    tagged_fixture_at_rate(path, 48000).await;
}

async fn tagged_fixture_at_rate(path: &Path, sample_rate: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut command = Command::new("ffmpeg");
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=96x64:rate=4",
        "-f",
        "lavfi",
        "-i",
        &format!("sine=frequency=440:sample_rate={sample_rate}"),
        "-t",
        "0.5",
        "-c:v",
        "mpeg4",
        "-threads:v",
        "1",
        "-c:a",
        "aac",
        "-movflags",
        "+use_metadata_tags",
        "-metadata:s:v:0",
        "handler_name=private video handler",
        "-metadata:s:v:0",
        "language=fra",
        "-metadata:s:a:0",
        "handler_name=private audio handler",
        "-metadata:s:a:0",
        "language=deu",
        "-metadata:s",
        "creation_time=2024-01-02T03:04:05.000000Z",
    ]);
    for (key, value) in GLOBAL_TAGS {
        command.arg("-metadata").arg(format!("{key}={value}"));
    }
    run(command.arg(path)).await;
    let metadata = probe(path).await;
    for (key, value) in GLOBAL_TAGS {
        assert_eq!(
            metadata["format"]["tags"][key], *value,
            "fixture lacks {key}"
        );
    }
    for (index, handler, language) in [
        (0, "private video handler", "fra"),
        (1, "private audio handler", "deu"),
    ] {
        assert_eq!(metadata["streams"][index]["tags"]["handler_name"], handler);
        assert_eq!(metadata["streams"][index]["tags"]["language"], language);
    }
}

fn assert_sanitized(metadata: &Value, sample_rate: u32) {
    for (key, value) in metadata["format"]["tags"].as_object().unwrap() {
        match key.as_str() {
            "major_brand" | "minor_version" | "compatible_brands" => {}
            "encoder" => assert!(value.as_str().unwrap().starts_with("Lavf")),
            _ => panic!("source container metadata survived: {key}={value}"),
        }
    }
    let streams = metadata["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2, "video and audio must both survive");
    for stream in streams {
        let handler = if stream["codec_type"] == "video" {
            assert_eq!(stream["codec_name"], "mpeg4", "video was re-encoded");
            assert_eq!(stream["width"], 96);
            assert_eq!(stream["height"], 64);
            "VideoHandler"
        } else {
            assert_eq!(stream["codec_type"], "audio");
            assert_eq!(stream["codec_name"], "aac");
            assert_eq!(stream["sample_rate"], sample_rate.to_string());
            "SoundHandler"
        };
        for (key, value) in stream["tags"].as_object().unwrap() {
            match key.as_str() {
                "language" => assert_eq!(value, "und"),
                "handler_name" => assert_eq!(value, handler),
                "vendor_id" => assert_eq!(value, "[0][0][0][0]"),
                "encoder" => assert!(value.as_str().unwrap().starts_with("Lavc")),
                _ => panic!("source stream metadata survived: {key}={value}"),
            }
        }
    }
}

async fn wait_before_replace(destination: &Path) -> PathBuf {
    let directory = destination.parent().unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(staged) = fs::read_to_string(directory.join(REACHED_REPLACE)) {
                return directory.join(staged);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("strip operation did not reach verified replacement within 20 seconds")
}

#[tokio::test]
async fn strips_existing_orphan_videos_without_changing_originals_or_media() {
    let root = tempfile::tempdir().unwrap();
    let original = root
        .path()
        .join("object/imported/01")
        .join(format!("{}.mp4", "01".repeat(32)));
    tagged_fixture(&original).await;
    let original_bytes = fs::read(&original).unwrap();
    let original_packets = packets(&original).await;
    let original_decoded = decoded_frames(&original).await;
    let clean = compressed_path(root.path(), 1);
    copy(&original, &clean);

    // This first run creates the clean candidate used by the mixed run below.
    let first = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (first.total, first.changed, first.unchanged, first.failed),
        (1, 1, 0, 0),
        "initial strip failed: {first:?}"
    );
    assert!(first.errors.is_empty());
    assert!(first.verification.passed);

    let tagged = [
        compressed_path(root.path(), 2),
        compressed_path(root.path(), 3),
    ];
    for path in &tagged {
        copy(&original, path);
    }
    let corrupt = compressed_path(root.path(), 4);
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(&corrupt, b"incomplete mp4 fixture").unwrap();
    let ignored = [
        clean.parent().unwrap().join(".orphan.metadata.tmp.mp4"),
        clean.parent().unwrap().join("not-a-hash.mp4"),
        clean.with_extension("jpg"),
    ];
    for path in &ignored {
        fs::write(path, b"unrelated fixture bytes").unwrap();
    }

    // No database exists: canonical compressed orphans still need sanitizing.
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            report.total,
            report.changed,
            report.unchanged,
            report.failed
        ),
        (4, 2, 1, 1)
    );
    assert!(!report.errors.is_empty());
    assert_eq!(report.verification.total, 4);
    assert_eq!(report.verification.checked, 3);
    assert_eq!(report.verification.location_tagged, 0);
    assert_eq!(report.verification.failed, 1);
    assert!(
        !report.verification.passed,
        "unreadable media is not privacy verified"
    );
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    assert_eq!(fs::read(&corrupt).unwrap(), b"incomplete mp4 fixture");
    for path in &ignored {
        assert_eq!(fs::read(path).unwrap(), b"unrelated fixture bytes");
    }
    let outputs = [&clean, &tagged[0], &tagged[1]];
    for path in outputs {
        assert_sanitized(&probe(path).await, 48000);
        assert_eq!(
            packets(path).await,
            original_packets,
            "compressed packets changed"
        );
        assert_eq!(
            decoded_frames(path).await,
            original_decoded,
            "playback changed"
        );
    }
    let clean_bytes = outputs.map(|path| fs::read(path).unwrap());
    let second = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            second.total,
            second.changed,
            second.unchanged,
            second.failed
        ),
        (4, 0, 3, 1)
    );
    for (path, bytes) in outputs.into_iter().zip(clean_bytes) {
        assert_eq!(fs::read(path).unwrap(), bytes, "clean rerun rewrote media");
    }
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
}

#[tokio::test]
async fn video_only_rotation_and_playback_survive_stream_copy() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tagged-fixture.mp4");
    tagged_fixture(&source).await;
    let destination = compressed_path(root.path(), 8);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    let help = run(Command::new("ffmpeg").args(["-hide_banner", "-h", "full"])).await;
    let has_display_rotation = String::from_utf8_lossy(&help.stdout).contains("-display_rotation");
    let mut rotate = Command::new("ffmpeg");
    rotate.args(["-v", "error", "-nostdin"]);
    if has_display_rotation {
        rotate.args(["-display_rotation:v:0", "90"]);
    }
    rotate.arg("-i").arg(&source).args([
        "-map",
        "0:v:0",
        "-c",
        "copy",
        "-movflags",
        "+use_metadata_tags",
    ]);
    if !has_display_rotation {
        rotate.args(["-metadata:s:v:0", "rotate=90"]);
    }
    run(rotate.arg(&destination)).await;
    let before = probe(&destination).await;
    assert_eq!(before["streams"].as_array().unwrap().len(), 1);
    assert!(
        before["streams"][0]["side_data_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|side_data| side_data["rotation"].is_i64())
    );
    let payload = packets(&destination).await;
    let playback = decoded_frames(&destination).await;
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (report.total, report.changed, report.failed),
        (1, 1, 0),
        "{report:?}"
    );
    assert!(report.verification.passed);
    let after = probe(&destination).await;
    assert_eq!(after["streams"].as_array().unwrap().len(), 1);
    assert_eq!(
        before["streams"][0]["side_data_list"],
        after["streams"][0]["side_data_list"]
    );
    assert_eq!(packets(&destination).await, payload);
    assert_eq!(decoded_frames(&destination).await, playback);
}

#[tokio::test]
async fn nested_and_versioned_videos_are_sanitized_but_scratch_files_are_ignored() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tagged-fixture.mp4");
    tagged_fixture(&source).await;
    let hash = "09".repeat(32);
    let nested = root.path().join("object/compressed/old/import");
    let destinations = [
        nested.join(format!("{hash}.mp4")),
        nested.join("versioned").join(format!("{hash}-v42.mp4")),
    ];
    for destination in &destinations {
        copy(&source, destination);
    }
    let ignored = [
        nested.join(format!(".{hash}.orphan.metadata.tmp.mp4")),
        nested.join(format!("{hash}-vnotnumber.mp4")),
        nested.join("arbitrary.mp4"),
    ];
    for path in &ignored {
        fs::write(path, b"ignored fixture bytes").unwrap();
    }
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            report.total,
            report.changed,
            report.unchanged,
            report.failed
        ),
        (2, 2, 0, 0)
    );
    assert_eq!(report.verification.checked, 2);
    assert!(report.verification.passed);
    for destination in &destinations {
        assert_sanitized(&probe(destination).await, 48000);
    }
    for path in &ignored {
        assert_eq!(fs::read(path).unwrap(), b"ignored fixture bytes");
    }
    let clean_bytes = destinations.each_ref().map(|path| fs::read(path).unwrap());
    let second = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            second.total,
            second.changed,
            second.unchanged,
            second.failed
        ),
        (2, 0, 2, 0)
    );
    for (destination, bytes) in destinations.iter().zip(clean_bytes) {
        assert_eq!(fs::read(destination).unwrap(), bytes);
    }
}

#[tokio::test]
async fn positive_audio_video_offsets_keep_exact_packet_timing() {
    assert_offset_timing("2", 48000).await;
}

#[tokio::test]
async fn fractional_audio_video_offsets_keep_exact_packet_timing() {
    assert_offset_timing("2.123456", 48000).await;
}

#[tokio::test]
async fn audio_at_44100_hz_keeps_exact_packet_timing() {
    assert_offset_timing("2.123456", 44100).await;
}

async fn assert_offset_timing(offset_seconds: &str, sample_rate: u32) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("tagged-fixture.mp4");
    tagged_fixture_at_rate(&source, sample_rate).await;
    let offset = root.path().join("offset-fixture.mp4");
    run(Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-copyts",
            "-itsoffset",
            offset_seconds,
            "-i",
        ])
        .arg(&source)
        .args(["-map", "0", "-c", "copy", "-movflags", "+use_metadata_tags"])
        .arg(&offset))
    .await;
    let destination = compressed_path(root.path(), 10);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    // Do not pre-remux the fixture: that hides the AAC final partial-packet
    // trim which the first metadata cleanup must preserve.
    copy(&offset, &destination);
    let before = probe(&destination).await;
    for stream in before["streams"].as_array().unwrap() {
        assert!(stream["start_pts"].as_i64().unwrap() > 0);
    }
    let timing = packets(&destination).await;
    let decoded = decoded_frames(&destination).await;
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (report.total, report.changed, report.failed),
        (1, 1, 0),
        "positive offsets need copied timestamps: {:?}",
        report.errors
    );
    assert!(report.verification.passed);
    let after = probe(&destination).await;
    assert_sanitized(&after, sample_rate);
    for (before, after) in before["streams"]
        .as_array()
        .unwrap()
        .iter()
        .zip(after["streams"].as_array().unwrap())
    {
        for key in [
            "start_pts",
            "start_time",
            "time_base",
            "duration",
            "duration_ts",
        ] {
            assert_eq!(before[key], after[key], "stream timing changed: {key}");
        }
    }
    assert_eq!(packets(&destination).await, timing);
    assert_eq!(decoded_frames(&destination).await, decoded);
}

#[test]
fn playback_verification_rejects_changed_timing_and_codec_extradata() {
    let before = serde_json::json!({
        "format": { "duration": "1.0" },
        "streams": [{
            "time_base": "1/48000", "start_pts": 96000, "duration_ts": 48000,
            "extradata_hash": "SHA256:original-fixture-codec-data",
        }],
    });
    super::probe::verify_playback(&before, &before).unwrap();
    for (key, value) in [
        ("time_base", serde_json::json!("1/24000")),
        ("start_pts", serde_json::json!(0)),
        ("duration_ts", serde_json::json!(48001)),
        (
            "extradata_hash",
            serde_json::json!("SHA256:changed-fixture-codec-data"),
        ),
    ] {
        let mut after = before.clone();
        after["streams"][0][key] = value;
        assert!(
            super::probe::verify_playback(&before, &after)
                .unwrap_err()
                .to_string()
                .contains(key)
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn compressed_symlink_never_follows_or_changes_original() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("object/imported/original-fixture.mp4");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    fs::write(&original, b"untouched original fixture").unwrap();
    let destination = compressed_path(root.path(), 11);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&original, &destination).unwrap();
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            report.total,
            report.changed,
            report.unchanged,
            report.failed
        ),
        (1, 0, 0, 1)
    );
    assert_eq!(report.verification.checked, 0);
    assert_eq!(report.verification.failed, 1);
    assert!(!report.verification.passed);
    assert!(
        fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&original).unwrap(), b"untouched original fixture");
}

#[tokio::test]
async fn failed_replace_keeps_source_and_postrun_verification_reports_location() {
    let root = tempfile::tempdir().unwrap();
    let destination = compressed_path(root.path(), 5);
    tagged_fixture(&destination).await;
    let original_bytes = fs::read(&destination).unwrap();
    fs::write(destination.parent().unwrap().join(FAIL_REPLACE), b"").unwrap();
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            report.total,
            report.changed,
            report.unchanged,
            report.failed
        ),
        (1, 0, 0, 1)
    );
    assert_eq!(report.errors.len(), 1);
    assert_eq!(fs::read(&destination).unwrap(), original_bytes);
    assert_eq!(report.verification.total, 1);
    assert_eq!(report.verification.checked, 1);
    assert_eq!(report.verification.location_tagged, 1);
    assert_eq!(report.verification.failed, 0);
    assert!(!report.verification.passed);
}

#[tokio::test]
async fn cancelling_before_replace_cleans_staging_and_preserves_source() {
    let root = tempfile::tempdir().unwrap();
    let destination = compressed_path(root.path(), 6);
    tagged_fixture(&destination).await;
    let original_bytes = fs::read(&destination).unwrap();
    let directory = destination.parent().unwrap();
    fs::write(directory.join(PAUSE_REPLACE), b"").unwrap();
    let task_root = root.path().to_path_buf();
    let task = tokio::spawn(async move { strip_compressed_videos(&task_root).await });
    let staged = wait_before_replace(&destination).await;
    assert!(staged.is_file());
    assert_sanitized(&probe(&staged).await, 48000);
    assert_eq!(fs::read(&destination).unwrap(), original_bytes);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        !staged.exists(),
        "cancelled operation leaked its staged file"
    );
    assert_eq!(fs::read(&destination).unwrap(), original_bytes);
    fs::remove_file(directory.join(PAUSE_REPLACE)).unwrap();
    let retry = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!((retry.total, retry.changed, retry.failed), (1, 1, 0));
    assert!(retry.verification.passed);
}

#[tokio::test]
async fn killed_process_leaves_original_intact_and_orphan_does_not_block_retry() {
    let current_directory = std::env::current_dir().unwrap();
    if current_directory.join(CHILD_MARKER).is_file() {
        let report = strip_compressed_videos(&current_directory).await.unwrap();
        panic!("child unexpectedly crossed the pre-replacement pause: {report:?}");
    }

    let root = tempfile::tempdir().unwrap();
    let destination = compressed_path(root.path(), 7);
    tagged_fixture(&destination).await;
    let original_bytes = fs::read(&destination).unwrap();
    let directory = destination.parent().unwrap();
    fs::write(directory.join(PAUSE_REPLACE), b"").unwrap();
    fs::write(root.path().join(CHILD_MARKER), b"").unwrap();
    let test_name = format!(
        "{}::killed_process_leaves_original_intact_and_orphan_does_not_block_retry",
        module_path!().split_once("::").unwrap().1
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_name, "--nocapture"])
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let staged = wait_before_replace(&destination).await;
    assert!(staged.is_file());
    child.start_kill().unwrap();
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
    assert_eq!(fs::read(&destination).unwrap(), original_bytes);
    assert!(
        staged.is_file(),
        "hard kill must exercise an orphaned staging file"
    );
    fs::remove_file(directory.join(PAUSE_REPLACE)).unwrap();
    let retry = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (retry.total, retry.changed, retry.unchanged, retry.failed),
        (1, 1, 0, 0)
    );
    assert!(retry.verification.passed);
    assert_sanitized(&probe(&destination).await, 48000);
    assert!(
        staged.is_file(),
        "retry must ignore unrelated/orphan staging files"
    );
}

#[tokio::test]
async fn missing_compressed_directory_is_an_empty_verified_run() {
    let root = tempfile::tempdir().unwrap();
    let report = strip_compressed_videos(root.path()).await.unwrap();
    assert_eq!(
        (
            report.total,
            report.changed,
            report.unchanged,
            report.failed
        ),
        (0, 0, 0, 0)
    );
    assert!(report.errors.is_empty());
    assert_eq!(report.verification.total, 0);
    assert_eq!(report.verification.checked, 0);
    assert_eq!(report.verification.location_tagged, 0);
    assert_eq!(report.verification.failed, 0);
    assert!(report.verification.passed);
}
