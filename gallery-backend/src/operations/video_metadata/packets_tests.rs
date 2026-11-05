//! Synthetic local media only; never opens an installed data directory.
use std::{path::Path, process::Output, time::Duration};

use serde_json::Value;
use tokio::{io::AsyncWriteExt, process::Command};

use super::{fingerprints, hash_output};

async fn run(command: &mut Command) -> Output {
    let output = tokio::time::timeout(Duration::from_secs(20), command.kill_on_drop(true).output())
        .await
        .expect("synthetic media command timed out")
        .unwrap();
    assert!(output.status.success(), "synthetic media command failed");
    assert!(output.stderr.is_empty(), "synthetic media diagnostics");
    output
}

async fn video(path: &Path, flip: bool) {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=96x64:rate=25",
        "-t",
        "0.6",
        "-c:v",
        "mpeg4",
        "-bf",
        "2",
        "-threads:v",
        "1",
    ]);
    if flip {
        command.args(["-vf", "hflip"]);
    }
    run(command.arg(path)).await;
}

async fn summary(path: &Path) -> Value {
    let output = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-count_packets",
            "-show_data_hash",
            "sha256",
            "-of",
            "json",
        ])
        .arg(path))
    .await;
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn packet_order(path: &Path) -> Vec<u8> {
    run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_packets",
            "-show_entries",
            "packet=stream_index",
            "-of",
            "compact=p=0:nk=0",
        ])
        .arg(path))
    .await
    .stdout
}

async fn payload_hashes(path: &Path) -> Vec<u8> {
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-map",
            "0",
            "-c",
            "copy",
            "-f",
            "streamhash",
            "-hash",
            "sha256",
            "-",
        ]))
    .await
    .stdout
}

#[tokio::test]
async fn rejects_interior_timing_changes_hidden_by_stream_summaries() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.mp4");
    let retimed = root.path().join("retimed.mp4");
    video(&source, false).await;
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-copyts", "-i"])
        .arg(&source)
        .args([
            "-map",
            "0",
            "-c",
            "copy",
            "-bsf:v",
            "setts=pts='if(eq(N,4),PTS+64,PTS)':dts=DTS",
            "-movflags",
            "+faststart",
        ])
        .arg(&retimed))
    .await;
    let before = summary(&source).await;
    let after = summary(&retimed).await;
    super::super::probe::verify_playback(&before, &after).unwrap();
    assert_eq!(
        payload_hashes(&source).await,
        payload_hashes(&retimed).await
    );
    let original = fingerprints(&source, 1).await.unwrap();
    assert_ne!(original, fingerprints(&retimed, 1).await.unwrap());

    let changed_payload = root.path().join("changed-payload.mp4");
    video(&changed_payload, true).await;
    assert_ne!(original, fingerprints(&changed_payload, 1).await.unwrap());
}

#[tokio::test]
async fn preserves_fingerprints_across_changed_interleaving_and_container_metadata() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.mp4");
    let remuxed = root.path().join("remuxed.mp4");
    // An integral AAC-packet duration isolates interleaving from tail trimming.
    run(Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "0.64",
            "-c:v",
            "mpeg4",
            "-threads:v",
            "1",
            "-c:a",
            "aac",
            "-metadata",
            "title=synthetic private metadata",
        ])
        .arg(&source))
    .await;
    run(Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-copyts", "-i"])
        .arg(&source)
        .args([
            "-map",
            "0",
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-audio_preload",
            "500000",
            "-movflags",
            "+faststart",
        ])
        .arg(&remuxed))
    .await;
    assert_ne!(packet_order(&source).await, packet_order(&remuxed).await);
    let packet_details = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "1",
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            super::PACKET_ENTRIES,
            "-of",
            "compact=p=0:nk=0",
        ])
        .arg(&source))
    .await;
    let packet_details = String::from_utf8(packet_details.stdout).unwrap();
    assert!(packet_details.contains("side_data_type=Skip Samples"));
    assert!(packet_details.contains("skip_samples=1024"));
    assert!(packet_details.contains("discard_padding=0"));
    assert_eq!(
        fingerprints(&source, 2).await.unwrap(),
        fingerprints(&remuxed, 2).await.unwrap(),
    );
}

#[tokio::test]
async fn includes_tail_duration_and_skip_discard_samples_without_chunk_dependence() {
    let packet = b"pts=0|dts=0|duration=448|size=3|flags=K_|data_hash=SHA256:fixture|side_data_type=Skip Samples|skip_samples=1024|discard_padding=576\n";
    let original = hash_output(packet.as_slice()).await.unwrap().0;
    for (from, to) in [
        ("duration=448", "duration=1024"),
        ("skip_samples=1024", "skip_samples=0"),
        ("discard_padding=576", "discard_padding=0"),
        ("flags=K_", "flags=KD"),
        ("SHA256:fixture", "SHA256:changed"),
    ] {
        let changed = String::from_utf8_lossy(packet).replace(from, to);
        assert_ne!(original, hash_output(changed.as_bytes()).await.unwrap().0);
    }
    let (mut writer, reader) = tokio::io::duplex(64);
    let producer = async {
        for _ in 0..1000 {
            writer.write_all(packet).await.unwrap();
        }
        drop(writer);
    };
    let ((), (hash, nonempty)) =
        tokio::join!(producer, async { hash_output(reader).await.unwrap() });
    let mut expected = blake3::Hasher::new();
    for _ in 0..1000 {
        expected.update(packet);
    }
    assert_eq!(hash, expected.finalize());
    assert!(nonempty);
    assert!(!hash_output(b" \n\t".as_slice()).await.unwrap().1);
}

#[tokio::test]
async fn rejects_empty_selections_and_suppresses_private_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.mp4");
    video(&source, false).await;
    assert!(fingerprints(&source, 0).await.is_err());
    assert!(fingerprints(&source, 2).await.is_err());
    let private = root.path().join("private-diagnostic-marker.mp4");
    std::fs::write(&private, b"invalid synthetic media").unwrap();
    let error = format!("{:#}", fingerprints(&private, 1).await.unwrap_err());
    assert_eq!(error, "packet verification command failed");
}
