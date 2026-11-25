//! Owner-only Florence suggestions. Authored feedback and Auto tags remain separate.
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    operations::embedding::catalog::{GalleryCatalog, Photo, PhotoCatalog},
    public::{constant::storage::get_data_path, db::tree::TREE},
    storage::suggestions::{FILE_NAME, SuggestedTags, SuggestionStore},
};

pub const VERSION: &str = "florence-large-ft-04ace749-frozen-v1";
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_REPLY: usize = 8192;
static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
static ALLOWED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        include_str!("../../../tools/suggestions/allowed-labels.txt"),
        include_str!("../../../tools/suggestions/extra-labels.txt"),
    ]
    .into_iter()
    .flat_map(str::lines)
    .filter_map(|line| line.split('|').next())
    .filter(|label| !["flower", "rose", "sunflower", "ground"].contains(label))
    .collect()
});

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub initialized: bool,
    pub total_photos: usize,
    pub pending_photos: usize,
    pub processing: bool,
    pub last_error: Option<&'static str>,
}

pub struct Service {
    pub store: SuggestionStore,
    status: Mutex<Status>,
    stopped: AtomicBool,
    #[cfg(unix)]
    active: Mutex<Option<std::os::unix::net::UnixStream>>,
}

impl Service {
    fn new(store: SuggestionStore) -> Self {
        Self {
            store,
            status: Mutex::new(Status::default()),
            stopped: AtomicBool::new(false),
            #[cfg(unix)]
            active: Mutex::new(None),
        }
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        if let Some(stream) = self.active.lock().unwrap().take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn current(&self, id: &str, version: u32) -> Result<Option<SuggestedTags>> {
        Ok(self
            .store
            .get(id)?
            .filter(|row| row.version == VERSION && row.thumbnail_version == version))
    }
}

pub fn service() -> Option<&'static Arc<Service>> {
    SERVICE.get()
}

#[cfg(test)]
pub(crate) fn install_test_service(store: SuggestionStore) {
    assert!(SERVICE.set(Arc::new(Service::new(store))).is_ok());
}

struct Pending {
    photo: Photo,
    urgent: bool,
    failures: u32,
    retry_at: Instant,
}

#[derive(Default)]
struct Queue {
    initialized: bool,
    prefer_favorite: bool,
    seen: HashMap<String, u32>,
    pending: HashMap<String, Pending>,
}

impl Queue {
    fn reconcile(&mut self, photos: Vec<Photo>, completed: &HashMap<String, u32>, now: Instant) {
        let current: HashMap<_, _> = photos
            .iter()
            .map(|photo| (photo.id.clone(), photo.thumbnail_version))
            .collect();
        self.pending.retain(|id, item| {
            current.get(id) == Some(&item.photo.thumbnail_version)
                && completed.get(id) != Some(&item.photo.thumbnail_version)
        });
        for photo in photos {
            if completed.get(&photo.id) == Some(&photo.thumbnail_version) {
                continue;
            }
            let urgent =
                self.initialized && self.seen.get(&photo.id) != Some(&photo.thumbnail_version);
            self.pending
                .entry(photo.id.clone())
                .and_modify(|item| item.photo = photo.clone())
                .or_insert(Pending {
                    photo,
                    urgent,
                    failures: 0,
                    retry_at: now,
                });
        }
        self.seen = current;
        self.initialized = true;
    }

    fn next(&self, now: Instant) -> Option<Photo> {
        self.pending
            .iter()
            .filter(|(_, item)| item.retry_at <= now)
            .min_by_key(|(id, item)| {
                (
                    !item.urgent,
                    item.photo.favorite != self.prefer_favorite,
                    Reverse(item.photo.timestamp),
                    item.retry_at,
                    *id,
                )
            })
            .map(|(_, item)| item.photo.clone())
    }
}

/// Shutdown interrupts the socket and joins this writer before the backup can proceed.
pub fn start() -> Option<thread::JoinHandle<()>> {
    let Some(socket) = std::env::var_os("UROCISSA_SUGGESTION_SOCKET") else {
        return None;
    };
    let socket = PathBuf::from(socket);
    if !socket.is_absolute() {
        return None;
    }
    let Ok(store) = SuggestionStore::open(&get_data_path().join("db").join(FILE_NAME)) else {
        log::warn!("Suggested tags are unavailable");
        return None;
    };
    let service = Arc::new(Service::new(store));
    if SERVICE.set(Arc::clone(&service)).is_err() {
        return None;
    }
    Some(thread::spawn(move || run(&socket, &service)))
}

pub fn stop() {
    if let Some(service) = service() {
        service.stop();
    }
}

fn run(socket: &Path, service: &Service) {
    let catalog = GalleryCatalog;
    let mut queue = Queue::default();
    while !service.stopped.load(Ordering::SeqCst) {
        let result = (|| -> Result<()> {
            let settings = TREE.store.auto_tag_settings()?;
            let photos = catalog
                .photos()?
                .into_iter()
                .filter_map(|photo| match settings.enabled(&photo.id) {
                    Ok(true) => Some(Ok(photo)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<Vec<_>>>()?;
            let completed = service
                .store
                .all()?
                .into_iter()
                .filter(|(_, row)| row.version == VERSION)
                .map(|(id, row)| (id, row.thumbnail_version))
                .collect();
            let now = Instant::now();
            queue.reconcile(photos, &completed, now);
            {
                let mut status = service.status.lock().unwrap();
                status.initialized = true;
                status.total_photos = queue.seen.len();
                status.pending_photos = queue.pending.len();
            }
            let Some(photo) = queue.next(now) else {
                thread::park_timeout(Duration::from_secs(5));
                return Ok(());
            };
            // Interleave newest favorites and newest other photos so neither
            // owner's favorites nor recent uploads wait behind an entire class.
            queue.prefer_favorite = !photo.favorite;
            service.status.lock().unwrap().processing = true;
            let analyzed = (|| -> Result<bool> {
                if !catalog.is_current(&photo)? {
                    return Ok(false);
                }
                let mut image = Vec::new();
                std::fs::File::open(&photo.thumbnail)?
                    .take(MAX_IMAGE + 1)
                    .read_to_end(&mut image)?;
                ensure!(
                    !image.is_empty() && image.len() as u64 <= MAX_IMAGE,
                    "invalid thumbnail"
                );
                let labels = request(socket, &image, service)?;
                if !catalog.is_current(&photo)?
                    || !TREE.store.auto_tag_settings()?.enabled(&photo.id)?
                {
                    return Ok(false);
                }
                service.store.put(
                    &photo.id,
                    &SuggestedTags {
                        version: VERSION.to_owned(),
                        thumbnail_version: photo.thumbnail_version,
                        labels,
                    },
                )?;
                Ok(true)
            })();
            let mut status = service.status.lock().unwrap();
            status.processing = false;
            if analyzed.is_ok() {
                queue.pending.remove(&photo.id);
                status.last_error = None;
            } else {
                let item = queue.pending.get_mut(&photo.id).expect("pending photo");
                item.failures = item.failures.saturating_add(1);
                item.retry_at = Instant::now()
                    + Duration::from_secs(2_u64.saturating_pow(item.failures.min(8)).min(300));
                status.last_error = Some("Suggested tags are temporarily unavailable");
            }
            status.pending_photos = queue.pending.len();
            Ok(())
        })();
        if result.is_err() {
            service.status.lock().unwrap().last_error =
                Some("Suggested tags are temporarily unavailable");
            thread::park_timeout(Duration::from_secs(5));
        }
    }
}

#[derive(Deserialize)]
struct Reply {
    candidates: Option<Vec<String>>,
}

#[cfg(unix)]
fn request(socket: &Path, image: &[u8], service: &Service) -> Result<Vec<String>> {
    use socket2::{Domain, SockAddr, Socket, Type};
    use std::os::unix::net::UnixStream;
    let connection = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    connection.connect_timeout(&SockAddr::unix(socket)?, Duration::from_secs(5))?;
    let mut stream: UnixStream = connection.into();
    stream.set_read_timeout(Some(Duration::from_secs(610)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    {
        let mut active = service.active.lock().unwrap();
        ensure!(!service.stopped.load(Ordering::SeqCst), "worker stopping");
        *active = Some(stream.try_clone()?);
    }
    let result = (|| {
        stream.write_all(&u32::try_from(image.len())?.to_be_bytes())?;
        stream.write_all(image)?;
        let mut header = [0; 4];
        stream.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header) as usize;
        ensure!(length > 0 && length <= MAX_REPLY, "invalid worker reply");
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes)?;
        let labels = serde_json::from_slice::<Reply>(&bytes)?
            .candidates
            .context("worker unavailable")?;
        ensure!(
            labels.len() <= 8 && labels.iter().all(|label| ALLOWED.contains(label.as_str())),
            "invalid worker reply"
        );
        Ok(labels)
    })();
    service.active.lock().unwrap().take();
    result
}

#[cfg(not(unix))]
fn request(_socket: &Path, _image: &[u8], _service: &Service) -> Result<Vec<String>> {
    anyhow::bail!("local worker unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(id: &str, version: u32) -> Photo {
        Photo {
            id: id.into(),
            thumbnail_version: version,
            thumbnail: PathBuf::new(),
            timestamp: 0,
            favorite: false,
        }
    }

    fn prioritized(id: &str, timestamp: i64, favorite: bool) -> Photo {
        Photo {
            timestamp,
            favorite,
            ..photo(id, 1)
        }
    }

    #[test]
    fn queue_interleaves_recent_and_favorites_and_refreshes_metadata() {
        let now = Instant::now();
        let mut queue = Queue::default();
        let photos = vec![
            prioritized("old-favorite", 10, true),
            prioritized("new-favorite", 20, true),
            prioritized("old-other", 30, false),
            prioritized("new-other", 40, false),
        ];
        queue.reconcile(photos.clone(), &HashMap::new(), now);
        // Initial default is recent; after each choice prefer the other class.
        for expected in ["new-other", "new-favorite", "old-other", "old-favorite"] {
            let next = queue.next(now).unwrap();
            assert_eq!(next.id, expected);
            queue.prefer_favorite = !next.favorite;
            queue.pending.remove(&next.id);
        }

        // A favorite/date edit on a still-pending photo must replace its old
        // priority metadata without treating the unchanged photo as an upload.
        queue.reconcile(photos.clone(), &HashMap::new(), now);
        let mut changed = photos;
        changed[0].favorite = false;
        changed[0].timestamp = 50;
        queue.reconcile(changed, &HashMap::new(), now);
        queue.prefer_favorite = false;
        assert_eq!(queue.next(now).unwrap().id, "old-favorite");
        queue.pending.get_mut("old-favorite").unwrap().retry_at = now + Duration::from_secs(1);
        assert_eq!(queue.next(now).unwrap().id, "new-other");
    }

    #[test]
    fn queue_resumes_and_prioritizes_new_uploads() {
        let now = Instant::now();
        let mut queue = Queue::default();
        queue.reconcile(
            vec![photo("a", 1), photo("b", 1)],
            &HashMap::from([("a".into(), 1)]),
            now,
        );
        assert_eq!(queue.next(now).unwrap().id, "b");
        queue.reconcile(
            vec![photo("a", 1), photo("b", 1), photo("c", 1)],
            &HashMap::from([("a".into(), 1)]),
            now,
        );
        assert_eq!(queue.next(now).unwrap().id, "c");
        queue.reconcile(
            vec![photo("a", 2), photo("b", 1), photo("c", 1)],
            &HashMap::from([("a".into(), 1), ("c".into(), 1)]),
            now,
        );
        assert_eq!(queue.next(now).unwrap().id, "a");
    }

    #[cfg(unix)]
    #[test]
    fn stop_interrupts_an_inflight_worker_read() -> Result<()> {
        use std::os::unix::net::UnixStream;
        let dir = tempfile::tempdir()?;
        let service = Service::new(SuggestionStore::open(&dir.path().join(FILE_NAME))?);
        let (mut reader, _worker) = UnixStream::pair()?;
        *service.active.lock().unwrap() = Some(reader.try_clone()?);
        let (done, received) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            let mut byte = [0];
            let _ = done.send(reader.read_exact(&mut byte).is_err());
        });
        service.stop();
        assert!(received.recv_timeout(Duration::from_secs(2))?);
        thread.join().unwrap();
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_cancels_the_real_framed_request() -> Result<()> {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("worker.sock");
        let listener = UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let service = Arc::new(Service::new(SuggestionStore::open(
            &dir.path().join(FILE_NAME),
        )?));
        let worker_service = Arc::clone(&service);
        let (done, received) = std::sync::mpsc::channel();
        let request_thread = thread::spawn(move || {
            let _ = done.send(request(&socket, b"fixture", &worker_service).is_err());
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut frame = [0_u8; 11];
        stream.read_exact(&mut frame)?;
        service.stop();
        assert!(received.recv_timeout(Duration::from_secs(2))?);
        request_thread.join().unwrap();
        Ok(())
    }
}
