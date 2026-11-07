//! Background photo embeddings. The authoritative queue is the durable photo library minus
//! current cached vectors, so losing this derived file or a process never loses pending work.
mod catalog;
pub mod client;
mod worker;

use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::Duration,
};

use serde::Serialize;

use crate::public::{constant::storage::get_data_path, structure::config::APP_CONFIG};
use crate::storage::embeddings::{EmbeddingStore, FILE_NAME};

use catalog::{GalleryCatalog, PhotoCatalog};
use client::HttpEmbedder;
use worker::Worker;

static SERVICE: OnceLock<Arc<EmbeddingService>> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingStatus {
    pub online: bool,
    pub offline_since: Option<i64>,
    pub model_id: Option<String>,
    pub device: Option<String>,
    pub total_photos: usize,
    pub embedded_photos: usize,
    pub pending_photos: usize,
    pub retrying_photos: usize,
    pub backfill_paused: bool,
    pub processing: bool,
    pub last_error: Option<String>,
}

pub struct EmbeddingService {
    status: Mutex<EmbeddingStatus>,
    paused: AtomicBool,
    stopped: AtomicBool,
    wake: SyncSender<()>,
}

impl EmbeddingService {
    fn new() -> (Arc<Self>, mpsc::Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                status: Mutex::new(EmbeddingStatus::default()),
                paused: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
                wake,
            }),
            receiver,
        )
    }

    pub fn status(&self) -> EmbeddingStatus {
        let mut status = self.status.lock().unwrap().clone();
        status.backfill_paused = self.paused.load(Ordering::Relaxed);
        status
    }

    pub fn pause_backfill(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        self.wake();
    }

    fn wake(&self) {
        // Coalesce bursts. Publishing a photo must never wait for an embedding or queue IO.
        let _ = self.wake.try_send(());
    }

    fn failure(&self, error: &anyhow::Error) {
        let message = format!("{error:#}");
        log::warn!("Photo embeddings: {message}");
        self.status.lock().unwrap().last_error = Some(message.chars().take(500).collect());
    }
}

pub fn service() -> Option<&'static Arc<EmbeddingService>> {
    SERVICE.get()
}

#[cfg(test)]
pub(crate) fn install_test_service() {
    assert!(SERVICE.set(EmbeddingService::new().0).is_ok());
}

pub fn wake() {
    if let Some(service) = service() {
        service.wake();
    }
}

/// Called after the initial tree is ready. Disabled means no worker, file or network traffic.
pub fn start() -> Option<JoinHandle<()>> {
    let configured = APP_CONFIG.get()?.read().ok()?.private.tagger_url.clone();
    let url = std::env::var("UROCISSA_TAGGER_URL").ok().or(configured)?;
    if url.trim().is_empty() {
        return None;
    }
    let (service, receiver) = EmbeddingService::new();
    if SERVICE.set(Arc::clone(&service)).is_err() {
        return None;
    }
    Some(std::thread::spawn(move || {
        let catalog: Arc<dyn PhotoCatalog> = Arc::new(GalleryCatalog);
        while !service.stopped.load(Ordering::Relaxed) {
            let initialized = (|| {
                let embedder = Arc::new(HttpEmbedder::new(&url)?);
                let store = EmbeddingStore::open(&get_data_path().join("db").join(FILE_NAME))?;
                Worker::new(Arc::clone(&catalog), embedder, store, Arc::clone(&service))
            })();
            match initialized {
                Ok(mut worker) => {
                    worker.run(&receiver);
                    break;
                }
                Err(error) => {
                    service.failure(&error);
                    let total = catalog.photos().map_or(0, |photos| photos.len());
                    let mut status = service.status.lock().unwrap();
                    status.total_photos = total;
                    status.pending_photos = total;
                    drop(status);
                    let _ = receiver.recv_timeout(Duration::from_secs(5));
                }
            }
        }
    }))
}

pub fn stop() {
    if let Some(service) = service() {
        service.stopped.store(true, Ordering::Relaxed);
        service.wake();
    }
}

#[cfg(test)]
mod tests;
