//! One explicitly requested photo, sent only to a configured local Unix socket.
use std::io::{Read, Write};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::public::{
    constant::storage::get_data_path,
    db::tree::TREE,
    structure::{
        abstract_data::{VIEWER_TIER, thumbnail_relative_path},
        object::ObjectType,
    },
};

const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_RESPONSE: usize = 8192;
const ALLOWED_LABELS: &str = include_str!("concept_candidates/allowed_labels.txt");

#[derive(Deserialize)]
struct Reply {
    candidates: Option<Vec<String>>,
}

/// Do not hold the gallery lock during disk IO or model inference.
pub fn for_photo(hash: &str) -> Result<Vec<String>> {
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid photo"
    );
    let (slot, version) = {
        let state = TREE
            .state
            .read()
            .map_err(|_| anyhow::anyhow!("gallery unavailable"))?;
        let slot = state.find(hash).context("photo unavailable")?;
        let record = state.get(slot).context("photo unavailable")?;
        ensure!(record.object_type == ObjectType::Image, "photos only");
        (slot, record.cache_version)
    };
    ensure!(
        TREE.store.auto_tag_settings()?.enabled(hash)?,
        "auto tags are disabled for this photo"
    );
    let path = get_data_path().join(thumbnail_relative_path(VIEWER_TIER, hash, version));
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_IMAGE + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_IMAGE,
        "invalid thumbnail size"
    );
    let labels = request(&bytes)?;
    {
        let state = TREE
            .state
            .read()
            .map_err(|_| anyhow::anyhow!("gallery unavailable"))?;
        ensure!(
            state
                .get(slot)
                .is_some_and(|record| record.id.as_str() == hash
                    && record.object_type == ObjectType::Image
                    && record.cache_version == version),
            "photo changed"
        );
    }
    ensure!(
        TREE.store.auto_tag_settings()?.enabled(hash)?,
        "auto tags are disabled for this photo"
    );
    Ok(labels)
}

#[cfg(unix)]
fn request(image: &[u8]) -> Result<Vec<String>> {
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    let path = std::env::var_os("UROCISSA_CONCEPT_SOCKET")
        .context("local concept worker is not configured")?;
    ensure!(
        Path::new(&path).is_absolute(),
        "worker socket must be an absolute path"
    );
    let mut stream = UnixStream::connect(path).context("local concept worker unavailable")?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    exchange(&mut stream, image)
}

#[cfg(not(unix))]
fn request(_image: &[u8]) -> Result<Vec<String>> {
    anyhow::bail!("local concept worker is available on Unix only")
}

fn exchange(stream: &mut (impl Read + Write), image: &[u8]) -> Result<Vec<String>> {
    ensure!(
        !image.is_empty() && image.len() as u64 <= MAX_IMAGE,
        "invalid image size"
    );
    stream.write_all(&u32::try_from(image.len())?.to_be_bytes())?;
    stream.write_all(image)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header) as usize;
    ensure!(
        size > 0 && size <= MAX_RESPONSE,
        "invalid worker response size"
    );
    let mut response = vec![0; size];
    stream.read_exact(&mut response)?;
    let reply: Reply = serde_json::from_slice(&response).context("invalid worker response")?;
    let labels = reply.candidates.context("local concept worker failed")?;
    ensure!(labels.len() <= 8, "too many candidates");
    ensure!(
        labels.iter().all(|label| ALLOWED_LABELS
            .lines()
            .any(|line| line.split('|').next() == Some(label.as_str()))),
        "invalid candidate"
    );
    Ok(labels)
}

#[cfg(test)]
mod tests;
