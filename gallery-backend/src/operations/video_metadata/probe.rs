use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use tokio::process::Command;

/// Bound every external process, kill it on cancellation, and never return raw
/// ffmpeg diagnostics (they can contain the private tags being removed).
pub(super) async fn run(command: &mut Command) -> Result<Vec<u8>> {
    let output = tokio::time::timeout(
        Duration::from_mins(5),
        command.stdin(Stdio::null()).kill_on_drop(true).output(),
    )
    .await
    .context("media command exceeded 300 seconds")?
    .context("could not run media command")?;
    ensure!(
        output.status.success(),
        "media command failed: {}",
        output.status
    );
    ensure!(output.stderr.is_empty(), "media command reported errors");
    Ok(output.stdout)
}

pub(super) async fn read(path: &Path) -> Result<Value> {
    ensure!(
        std::fs::symlink_metadata(path)?.file_type().is_file(),
        "compressed video is not a regular file"
    );
    let bytes = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-show_chapters",
            "-count_packets",
            "-show_data_hash",
            "sha256",
            "-of",
            "json",
        ])
        .arg(path))
    .await?;
    let probe: Value = serde_json::from_slice(&bytes).context("invalid media probe")?;
    let streams = probe["streams"].as_array().context("no media streams")?;
    ensure!(
        streams.iter().any(|s| s["codec_type"] == "video"),
        "compressed video has no video stream"
    );
    ensure!(
        streams.iter().all(|s| s["nb_read_packets"]
            .as_str()
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|n| n > 0)),
        "compressed video contains an empty or unreadable stream"
    );
    duration(&probe["format"])?;
    Ok(probe)
}

fn duration(value: &Value) -> Result<f64> {
    let duration: f64 = value["duration"]
        .as_str()
        .context("missing duration")?
        .parse()
        .context("invalid duration")?;
    ensure!(duration.is_finite() && duration > 0.0, "invalid duration");
    Ok(duration)
}

pub(super) fn verify_playback(before: &Value, after: &Value) -> Result<()> {
    ensure!(
        (duration(&before["format"])? - duration(&after["format"])?).abs() <= 0.01,
        "container duration changed"
    );
    let before = before["streams"]
        .as_array()
        .context("missing source streams")?;
    let after = after["streams"]
        .as_array()
        .context("missing output streams")?;
    ensure!(before.len() == after.len(), "stream count changed");
    for (before, after) in before.iter().zip(after) {
        for key in [
            "codec_type",
            "codec_name",
            "profile",
            "width",
            "height",
            "pix_fmt",
            "sample_rate",
            "channels",
            "channel_layout",
            "nb_read_packets",
            "avg_frame_rate",
            "sample_aspect_ratio",
            "disposition",
            "side_data_list",
            "time_base",
            "start_pts",
            "duration_ts",
            "extradata_hash",
        ] {
            ensure!(before[key] == after[key], "stream property changed: {key}");
        }
        if before["duration"].is_string() {
            ensure!(
                (duration(before)? - duration(after)?).abs() <= 0.01,
                "stream duration changed"
            );
        }
    }
    Ok(())
}

/// Inspect container, stream, chapter and nested side-data tag names. Unknown
/// source tags are also rejected by the stricter sanitization allow-list.
pub(super) fn has_location(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            // Chroma sample placement is technical playback information.
            (key != "chroma_location"
                && ["location", "gps", "latitude", "longitude", "iso6709"]
                    .iter()
                    .any(|part| key.contains(part)))
                || has_location(value)
        }),
        Value::Array(values) => values.iter().any(has_location),
        _ => false,
    }
}
