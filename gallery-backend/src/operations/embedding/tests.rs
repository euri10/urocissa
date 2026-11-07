use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use tempfile::TempDir;

use super::{
    EmbeddingService,
    catalog::{Photo, PhotoCatalog},
    client::{Embedder, ImageEmbedding, ModelInfo},
    worker::Worker,
};
use crate::storage::embeddings::{EmbeddingStore, FILE_NAME};

#[derive(Default)]
struct Catalog(Mutex<Vec<Photo>>);

impl Catalog {
    fn add(&self, root: &Path, id: &str, version: u32) {
        let thumbnail = root.join(format!("{id}-{version}.jpg"));
        std::fs::write(&thumbnail, id).unwrap();
        let mut photos = self.0.lock().unwrap();
        photos.retain(|photo| photo.id != id);
        photos.push(Photo {
            id: id.into(),
            thumbnail_version: version,
            thumbnail,
        });
    }
}

impl PhotoCatalog for Catalog {
    fn photos(&self) -> Result<Vec<Photo>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn is_current(&self, photo: &Photo) -> Result<bool> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|current| current == photo))
    }
}

struct FakeEmbedder {
    available: AtomicBool,
    model: Mutex<ModelInfo>,
    calls: Mutex<Vec<String>>,
    failing_photo: Mutex<Option<String>>,
    gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl FakeEmbedder {
    fn new() -> Self {
        Self {
            available: AtomicBool::new(true),
            model: Mutex::new(ModelInfo {
                model_id: "model-a".into(),
                embedding_dim: 2,
                device: "CPU".into(),
            }),
            calls: Mutex::new(Vec::new()),
            failing_photo: Mutex::new(None),
            gate: Mutex::new(None),
        }
    }
}

impl Embedder for FakeEmbedder {
    fn info(&self) -> Result<ModelInfo> {
        ensure!(self.available.load(Ordering::Relaxed), "tagger offline");
        Ok(self.model.lock().unwrap().clone())
    }
    fn embed_image(&self, bytes: Vec<u8>) -> Result<ImageEmbedding> {
        ensure!(self.available.load(Ordering::Relaxed), "tagger offline");
        let id = String::from_utf8(bytes)?;
        self.calls.lock().unwrap().push(id.clone());
        if let Some((entered, release)) = self.gate.lock().unwrap().take() {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        ensure!(
            self.failing_photo.lock().unwrap().as_ref() != Some(&id),
            "bad photo"
        );
        Ok(ImageEmbedding {
            model_id: self.model.lock().unwrap().model_id.clone(),
            embedding: vec![0.6, 0.8],
        })
    }
}

struct Fixture {
    root: TempDir,
    catalog: Arc<Catalog>,
    embedder: Arc<FakeEmbedder>,
    service: Arc<EmbeddingService>,
}

impl Fixture {
    fn new(ids: &[&str]) -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            catalog: Arc::new(Catalog::default()),
            embedder: Arc::new(FakeEmbedder::new()),
            service: EmbeddingService::new().0,
        };
        for id in ids {
            fixture.add(id, 0);
        }
        fixture
    }
    fn add(&self, id: &str, version: u32) {
        self.catalog.add(self.root.path(), id, version);
    }
    fn path(&self) -> PathBuf {
        self.root.path().join(FILE_NAME)
    }
    fn worker(&self) -> Worker {
        Worker::new(
            self.catalog.clone(),
            self.embedder.clone(),
            EmbeddingStore::open(&self.path()).unwrap(),
            self.service.clone(),
        )
        .unwrap()
    }
}

#[test]
fn offline_uploads_remain_pending_and_restart_resumes_completed_work() {
    let fixture = Fixture::new(&["a", "b"]);
    let mut worker = fixture.worker();
    let now = Instant::now();
    worker.step(now).unwrap();
    assert_eq!(fixture.service.status().embedded_photos, 1);
    fixture.embedder.available.store(false, Ordering::Relaxed);
    worker.step(now + Duration::from_secs(31)).unwrap();
    let offline_since = fixture.service.status().offline_since;
    assert!(offline_since.is_some());
    fixture.add("new-upload", 0);
    worker.reconcile(now).unwrap();
    assert_eq!(fixture.service.status().pending_photos, 2);
    worker.step(now + Duration::from_secs(34)).unwrap();
    assert_eq!(fixture.service.status().offline_since, offline_since);
    drop(worker);

    let mut resumed = fixture.worker();
    assert_eq!(fixture.service.status().embedded_photos, 1);
    fixture.embedder.available.store(true, Ordering::Relaxed);
    let now = Instant::now();
    resumed.step(now).unwrap();
    resumed.step(now).unwrap();
    assert_eq!(fixture.service.status().pending_photos, 0);
    assert_eq!(fixture.service.status().offline_since, None);
    assert_eq!(
        fixture
            .embedder
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|id| id.as_str() == "a")
            .count(),
        1
    );
}

#[test]
fn deleting_derived_file_rebuilds_every_photo_without_touching_v6() {
    let fixture = Fixture::new(&["a", "b"]);
    // The old gallery's DataStore can open the same V6 database before and after embedding.
    let main_path = fixture.root.path().join("data_v6.redb");
    let records: Vec<_> = (0..10)
        .map(|index| {
            use crate::public::structure::{
                abstract_data::AbstractData,
                image::{ImageCombined, ImageMetadata},
                object::{ObjectSchema, ObjectType},
            };
            let id = arrayvec::ArrayString::from(&format!("{index:064x}")).unwrap();
            let mut object = ObjectSchema::new(id, ObjectType::Image);
            object.tags.insert("owner-authored".into());
            AbstractData::Image(ImageCombined {
                object,
                metadata: ImageMetadata::new(id, 100, 20, 30, "jpg".into()),
            })
        })
        .collect();
    {
        let store = crate::storage::DataStore::initialize_empty(&main_path).unwrap();
        store
            .write(|writer| {
                for record in &records {
                    writer.insert(record)?;
                }
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
    }
    let before = std::fs::read(&main_path).unwrap();
    {
        let mut worker = fixture.worker();
        let now = Instant::now();
        worker.step(now).unwrap();
        worker.step(now).unwrap();
        assert_eq!(fixture.service.status().pending_photos, 0);
    }
    assert_eq!(std::fs::read(&main_path).unwrap(), before);
    std::fs::remove_file(fixture.path()).unwrap();
    let mut rebuilt = fixture.worker();
    assert_eq!(fixture.service.status().pending_photos, 2);
    let now = Instant::now();
    rebuilt.step(now).unwrap();
    rebuilt.step(now).unwrap();
    assert_eq!(fixture.service.status().pending_photos, 0);
    let legacy_reader = crate::storage::DataStore::open(&main_path).unwrap();
    let reader = legacy_reader.reader().unwrap();
    assert_eq!(reader.len().unwrap(), 10);
    for expected in records {
        let actual = reader
            .get(expected.hash().as_str())
            .unwrap()
            .unwrap()
            .into_value();
        assert_eq!(
            crate::storage::v6::V6AbstractData::from(actual),
            crate::storage::v6::V6AbstractData::from(expected)
        );
    }
}

#[test]
fn new_uploads_pass_backfill_and_pause_without_waiting_for_slow_inference() {
    let fixture = Fixture::new(&["a", "b", "c"]);
    let mut worker = fixture.worker();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *fixture.embedder.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let handle = std::thread::spawn(move || {
        worker.step(Instant::now()).unwrap();
        worker
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    fixture.add("z-new", 0);
    fixture.service.pause_backfill(true);
    fixture.service.wake(); // This must return while the inference call is still blocked.
    assert!(fixture.service.status().processing);
    release_tx.send(()).unwrap();
    let mut worker = handle.join().unwrap();
    let now = Instant::now();
    worker.reconcile(now).unwrap();
    assert_eq!(worker.step(now).unwrap(), Some(true));
    assert_eq!(*fixture.embedder.calls.lock().unwrap(), vec!["a", "z-new"]);
    assert_eq!(worker.step(now).unwrap(), None);
    fixture.service.pause_backfill(false);
    worker.step(now).unwrap();
    worker.step(now).unwrap();
    assert_eq!(fixture.service.status().pending_photos, 0);
}

#[test]
fn model_and_thumbnail_changes_reembed_but_metadata_changes_do_not() {
    let fixture = Fixture::new(&["a"]);
    let mut worker = fixture.worker();
    let now = Instant::now();
    worker.step(now).unwrap();
    worker.reconcile(now).unwrap();
    assert_eq!(worker.step(now).unwrap(), None);
    fixture.add("a", 1);
    worker.reconcile(now).unwrap();
    worker.step(now).unwrap();
    assert_eq!(
        worker
            .store
            .get("a", "model-a", 0)
            .unwrap()
            .unwrap()
            .thumbnail_version,
        1
    );
    fixture.embedder.model.lock().unwrap().model_id = "model-b".into();
    worker.step(now + Duration::from_secs(31)).unwrap();
    assert!(worker.store.get("a", "model-b", 0).unwrap().is_some());
    assert_eq!(fixture.embedder.calls.lock().unwrap().len(), 3);
}

#[test]
fn failing_photo_backs_off_and_does_not_starve_other_photos() {
    let fixture = Fixture::new(&["a", "b"]);
    *fixture.embedder.failing_photo.lock().unwrap() = Some("a".into());
    let mut worker = fixture.worker();
    let now = Instant::now();
    worker.step(now).unwrap();
    worker.step(now).unwrap();
    assert_eq!(*fixture.embedder.calls.lock().unwrap(), vec!["a", "b"]);
    assert_eq!(worker.step(now).unwrap(), None);
    assert_eq!(fixture.service.status().retrying_photos, 1);
    *fixture.embedder.failing_photo.lock().unwrap() = None;
    worker.step(now + Duration::from_secs(3)).unwrap();
    assert_eq!(fixture.service.status().pending_photos, 0);
}

#[test]
fn changed_or_deleted_photo_is_not_committed_after_inference() {
    for delete in [false, true] {
        let fixture = Fixture::new(&["a"]);
        let mut worker = fixture.worker();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        *fixture.embedder.gate.lock().unwrap() = Some((entered_tx, release_rx));
        let handle = std::thread::spawn(move || {
            worker.step(Instant::now()).unwrap();
            worker
        });
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        if delete {
            fixture.catalog.0.lock().unwrap().clear();
        } else {
            fixture.add("a", 1);
        }
        release_tx.send(()).unwrap();
        let worker = handle.join().unwrap();
        assert!(worker.store.get("a", "model-a", 0).unwrap().is_none());
        assert_eq!(
            fixture.service.status().pending_photos,
            usize::from(!delete)
        );
    }
}

#[test]
fn old_configuration_defaults_to_disabled_and_url_stays_private() {
    let mut json =
        serde_json::to_value(crate::public::structure::config::AppConfig::default()).unwrap();
    json["private"].as_object_mut().unwrap().remove("taggerUrl");
    let mut config: crate::public::structure::config::AppConfig =
        serde_json::from_value(json).unwrap();
    assert!(config.private.tagger_url.is_none());
    config.private.tagger_url = Some("http://tagger:5674".into());
    let public = serde_json::to_string(&config.public).unwrap();
    assert!(!public.contains("tagger"));
}
