use std::{io, path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, ensure};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const PACKET_ENTRIES: &str = "packet=pts,dts,duration,size,flags,data_hash:packet_side_data";

async fn hash_output(mut output: impl AsyncRead + Unpin) -> io::Result<(blake3::Hash, bool)> {
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0_u8; 8192];
    let mut nonempty = false;
    loop {
        let count = output.read(&mut buffer).await?;
        if count == 0 {
            return Ok((hash.finalize(), nonempty));
        }
        let bytes = &buffer[..count];
        nonempty |= bytes.iter().any(|byte| !byte.is_ascii_whitespace());
        hash.update(bytes);
    }
}

/// Include packet timing, payload and side data without retaining private media
/// diagnostics or an unbounded packet list. Selecting each stream independently
/// makes harmless changes in inter-stream mux ordering irrelevant.
pub(super) async fn fingerprints(path: &Path, stream_count: usize) -> Result<Vec<blake3::Hash>> {
    ensure!(stream_count > 0, "no streams to fingerprint");
    let mut fingerprints = Vec::with_capacity(stream_count);
    for index in 0..stream_count {
        let hash = tokio::time::timeout(Duration::from_mins(5), async {
            let mut child = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_packets",
                    "-show_data_hash",
                    "sha256",
                    "-show_entries",
                    PACKET_ENTRIES,
                    "-of",
                    "compact=p=0:nk=0",
                    "-select_streams",
                ])
                .arg(index.to_string())
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .context("could not start packet verification")?;
            let stdout = child.stdout.take().context("missing packet output")?;
            let mut stderr = child.stderr.take().context("missing packet diagnostics")?;
            let mut discarded = tokio::io::sink();
            let ((hash, nonempty), diagnostic_bytes, status) = tokio::try_join!(
                hash_output(stdout),
                tokio::io::copy(&mut stderr, &mut discarded),
                child.wait(),
            )
            .context("could not read packet verification")?;
            ensure!(status.success(), "packet verification command failed");
            ensure!(diagnostic_bytes == 0, "packet verification reported errors");
            ensure!(nonempty, "stream contains no packet fingerprints");
            Ok::<_, anyhow::Error>(hash)
        })
        .await
        .context("packet verification exceeded 300 seconds")??;
        fingerprints.push(hash);
    }
    Ok(fingerprints)
}

#[cfg(test)]
#[path = "packets_tests.rs"]
mod tests;
