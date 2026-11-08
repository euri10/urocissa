use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{Arc, atomic::Ordering, mpsc::Receiver},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};

use super::{
    EmbeddingService,
    catalog::{Photo, PhotoCatalog},
    client::{Embedder, ModelInfo},
};
use crate::storage::embeddings::{EmbeddingStore, StoredEmbedding};

const PROBE_INTERVAL: Duration = Duration::from_secs(30);
const SCAN_INTERVAL: Duration = Duration::from_secs(5);

struct PendingPhoto {
    photo: Photo,
    urgent: bool,
    failures: u32,
    retry_at: Instant,
}

pub(super) struct Worker {
    catalog: Arc<dyn PhotoCatalog>,
    embedder: Arc<dyn Embedder>,
    pub(super) store: EmbeddingStore,
    service: Arc<EmbeddingService>,
    model: Option<ModelInfo>,
    seen: HashMap<String, u32>,
    pending: BTreeMap<String, PendingPhoto>,
    initialized: bool,
    next_probe: Instant,
    probe_failures: u32,
}

fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(2_u64.saturating_pow(failures.min(8)).min(300))
}

impl Worker {
    pub(super) fn new(
        catalog: Arc<dyn PhotoCatalog>,
        embedder: Arc<dyn Embedder>,
        store: EmbeddingStore,
        service: Arc<EmbeddingService>,
    ) -> Result<Self> {
        let model = store.model()?;
        if let Some(model) = &model {
            model.validate()?;
        }
        let mut worker = Self {
            catalog,
            embedder,
            store,
            service,
            model,
            seen: HashMap::new(),
            pending: BTreeMap::new(),
            initialized: false,
            next_probe: Instant::now(),
            probe_failures: 0,
        };
        worker.reconcile(Instant::now())?;
        // Restore prepared Concepts offline too. A cache miss is retried after probing.
        worker.refresh_tags(true);
        Ok(worker)
    }

    pub(super) fn reconcile(&mut self, now: Instant) -> Result<()> {
        let photos = self.catalog.photos()?;
        let completed = self
            .model
            .as_ref()
            .map(|model| self.store.versions(&model.model_id))
            .transpose()?
            .unwrap_or_default();
        let current: HashMap<_, _> = photos
            .iter()
            .map(|photo| (photo.id.clone(), photo.thumbnail_version))
            .collect();
        self.pending.retain(|id, pending| {
            current.get(id) == Some(&pending.photo.thumbnail_version)
                && completed.get(id) != Some(&pending.photo.thumbnail_version)
        });
        for photo in photos {
            if completed.get(&photo.id) == Some(&photo.thumbnail_version) {
                continue;
            }
            let urgent =
                self.initialized && self.seen.get(&photo.id) != Some(&photo.thumbnail_version);
            self.pending
                .entry(photo.id.clone())
                .or_insert(PendingPhoto {
                    photo,
                    urgent,
                    failures: 0,
                    retry_at: now,
                });
        }
        self.seen = current;
        self.initialized = true;
        self.update_counts();
        self.refresh_tags(false);
        Ok(())
    }

    fn refresh_tags(&self, prepare: bool) {
        if let Some(tags) = self.service.auto_tags.get() {
            let result = (|| {
                if prepare && let Some(model) = &self.model {
                    tags.prepare(model, self.embedder.as_ref())?;
                }
                tags.reconcile()
            })();
            if let Err(error) = result {
                self.service.failure(&error);
            }
        }
    }

    fn update_counts(&self) {
        let mut status = self.service.status.lock().unwrap();
        status.total_photos = self.seen.len();
        status.pending_photos = self.pending.len();
        status.embedded_photos = self.seen.len().saturating_sub(self.pending.len());
        status.retrying_photos = self
            .pending
            .values()
            .filter(|item| item.failures > 0)
            .count();
        status.model_id = self.model.as_ref().map(|model| model.model_id.clone());
        status.device = self.model.as_ref().map(|model| model.device.clone());
    }

    fn probe(&mut self, now: Instant) -> Result<()> {
        let result = self.embedder.info().and_then(|model| {
            model.validate()?;
            if let Some(previous) = &self.model {
                ensure!(
                    previous.model_id != model.model_id
                        || previous.embedding_dim == model.embedding_dim,
                    "tagger reused a model ID with a different dimension"
                );
            }
            Ok(model)
        });
        match result {
            Ok(model) => {
                let changed = self
                    .model
                    .as_ref()
                    .is_none_or(|old| old.model_id != model.model_id);
                self.store.set_model(&model)?;
                self.model = Some(model);
                self.refresh_tags(true);
                self.next_probe = now + PROBE_INTERVAL;
                self.probe_failures = 0;
                {
                    let mut status = self.service.status.lock().unwrap();
                    status.online = true;
                    status.offline_since = None;
                    if status.retrying_photos == 0 {
                        status.last_error = None;
                    }
                }
                if changed {
                    for item in self.pending.values_mut() {
                        item.failures = 0;
                        item.retry_at = now;
                    }
                    self.reconcile(now)?;
                }
                self.update_counts();
            }
            Err(error) => {
                self.probe_failures = self.probe_failures.saturating_add(1);
                self.next_probe = now + retry_delay(self.probe_failures);
                let mut status = self.service.status.lock().unwrap();
                status.online = false;
                status
                    .offline_since
                    .get_or_insert_with(|| chrono::Utc::now().timestamp_millis());
                drop(status);
                self.service.failure(&error);
            }
        }
        Ok(())
    }

    /// Process at most one photo. An in-flight backfill request finishes before a new upload;
    /// every subsequent selection prefers new or changed photos, including while paused.
    pub(super) fn step(&mut self, now: Instant) -> Result<Option<bool>> {
        if now >= self.next_probe {
            self.probe(now)?;
        }
        if !self.service.status.lock().unwrap().online {
            return Ok(None);
        }
        let paused = self.service.paused.load(Ordering::Relaxed);
        let candidate = self
            .pending
            .iter()
            .filter(|(_, item)| item.retry_at <= now && (item.urgent || !paused))
            .min_by_key(|(id, item)| (!item.urgent, item.retry_at, *id))
            .map(|(id, item)| (id.clone(), item.photo.clone(), item.urgent));
        let Some((id, photo, urgent)) = candidate else {
            return Ok(None);
        };
        let model = self.model.as_ref().expect("online worker has a model");
        self.service.status.lock().unwrap().processing = true;
        let result = (|| {
            if !self.catalog.is_current(&photo)? {
                return Ok(false);
            }
            let jpeg = read_thumbnail(&photo)?;
            let result = self.embedder.embed_image(jpeg)?;
            result.validate(model)?;
            // Rotation/reindex/deletion during a slow request cannot publish a stale vector.
            if !self.catalog.is_current(&photo)? {
                return Ok(false);
            }
            self.store.put(
                &photo.id,
                &model.model_id,
                0,
                &StoredEmbedding {
                    thumbnail_version: photo.thumbnail_version,
                    vector: result.embedding,
                },
            )?;
            Ok(true)
        })();
        self.service.status.lock().unwrap().processing = false;
        match result {
            Ok(completed) => {
                self.pending.remove(&id);
                if completed {
                    self.service.status.lock().unwrap().last_error = None;
                    if let Some(tags) = self.service.auto_tags.get() {
                        tags.photo_embedded(&id);
                    }
                    self.refresh_tags(false);
                } else {
                    self.reconcile(now)?;
                }
            }
            Err(error) => {
                let item = self.pending.get_mut(&id).expect("selected pending photo");
                item.failures = item.failures.saturating_add(1);
                item.retry_at = now + retry_delay(item.failures);
                // Distinguish an unavailable tagger from a broken individual thumbnail.
                self.next_probe = now;
                self.service.failure(&error);
            }
        }
        self.update_counts();
        Ok(Some(urgent))
    }

    pub(super) fn run(&mut self, wake: &Receiver<()>) {
        let mut reconcile = true;
        let mut next_scan = Instant::now();
        while !self.service.stopped.load(Ordering::Relaxed) {
            let now = Instant::now();
            let step = (|| {
                if reconcile || now >= next_scan {
                    self.reconcile(now)?;
                    next_scan = now + SCAN_INTERVAL;
                }
                self.step(now)
            })();
            let delay = match step {
                Ok(Some(true)) => Duration::ZERO,
                // Leave breathing room for gallery work and uploads during backfill.
                Ok(Some(false)) => Duration::from_millis(100),
                Ok(None) => Duration::from_secs(1),
                Err(error) => {
                    self.service.failure(&error);
                    Duration::from_secs(5)
                }
            };
            reconcile = wake.recv_timeout(delay).is_ok();
        }
    }
}

fn read_thumbnail(photo: &Photo) -> Result<Vec<u8>> {
    const MAX_IMAGE: u64 = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(&photo.thumbnail)
        .context("photo thumbnail is not available")?
        .take(MAX_IMAGE + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_IMAGE,
        "invalid thumbnail size"
    );
    Ok(bytes)
}
