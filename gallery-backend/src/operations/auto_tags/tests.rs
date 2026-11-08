use super::*;
use crate::operations::embedding::client::ImageEmbedding;
use crate::storage::{DataStore, embeddings::StoredEmbedding};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fake(AtomicUsize);
impl Embedder for Fake {
    fn info(&self) -> Result<ModelInfo> {
        unreachable!()
    }
    fn embed_image(&self, _: Vec<u8>) -> Result<ImageEmbedding> {
        panic!("photos must not be re-embedded")
    }
    fn embed_text(&self, _: &str) -> Result<ImageEmbedding> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(ImageEmbedding {
            model_id: BuiltinCatalog::embedded()?.model_id,
            embedding: vec![1.0, 0.0],
        })
    }
}

#[test]
fn cached_concepts_rematerialize_without_photo_or_text_inference() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let store = EmbeddingStore::open(&dir.path().join("derived.redb"))?;
    let tags = AutoTags::new(store.clone())?;
    let model = ModelInfo {
        model_id: tags.catalog.model_id.clone(),
        embedding_dim: 2,
        device: "CPU".into(),
        logit_scale: Some(10.0),
        logit_bias: Some(0.0),
    };
    store.set_model(&model)?;
    let fake = Fake(AtomicUsize::new(0));
    tags.prepare(&model, &fake)?;
    assert_eq!(fake.0.load(Ordering::Relaxed), tags.catalog.concepts.len());
    let input = |enabled| {
        HashMap::from([(
            "photo".into(),
            Input {
                version: 3,
                tags: vec![],
                enabled,
                rejections: vec![],
            },
        )])
    };
    tags.materialize(&input(true), &[])?;
    assert!(tags.status("photo", 3, true)?.auto_tags_pending);
    store.put(
        "photo",
        &model.model_id,
        0,
        &StoredEmbedding {
            thumbnail_version: 3,
            vector: vec![1.0, 0.0],
        },
    )?;
    tags.photo_embedded("photo");
    tags.materialize(&input(true), &[])?;
    let original = tags.status("photo", 3, true)?.auto_tags;
    assert!(!original.is_empty());
    assert!(original.iter().any(|t| t.displayed));
    assert!(tags.status("photo", 4, true)?.auto_tags_pending);
    assert!(tags.status("photo", 3, false)?.auto_tags.is_empty());
    tags.materialize(&input(false), &[])?;
    assert!(tags.status("photo", 3, true)?.auto_tags_pending);
    tags.materialize(&input(true), &[])?;
    assert_eq!(tags.status("photo", 3, true)?.auto_tags, original);
    // Library aliases and per-photo User tags rematerialize without inference.
    tags.materialize(&input(true), &[("Chat".into(), 1)])?;
    assert!(
        tags.status("photo", 3, true)?
            .auto_tags
            .iter()
            .any(|t| t.concept == "cat" && t.label == "Chat")
    );
    let mut aliased = input(true);
    aliased.get_mut("photo").unwrap().tags.push("cat".into());
    tags.materialize(&aliased, &[("Chat".into(), 1)])?;
    assert!(
        !tags
            .status("photo", 3, true)?
            .auto_tags
            .iter()
            .any(|t| t.concept == "cat")
    );
    tags.materialize(&input(true), &[])?;
    let restarted = AutoTags::new(store.clone())?;
    restarted.prepare(&model, &fake)?;
    assert_eq!(fake.0.load(Ordering::Relaxed), tags.catalog.concepts.len());
    // New vocabulary release reuses photo vectors, but prepares its Concepts once.
    let mut next = AutoTags::new(store)?;
    next.catalog.version.push_str("-next");
    next.prepare(&model, &fake)?;
    assert!(next.status("photo", 3, true)?.auto_tags_pending);
    next.materialize(&input(true), &[])?;
    assert_eq!(next.status("photo", 3, true)?.auto_tags, original);
    assert_eq!(
        fake.0.load(Ordering::Relaxed),
        tags.catalog.concepts.len() * 2
    );
    Ok(())
}

#[test]
fn preferences_survive_derived_file_loss_without_changing_v6() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("authored.redb");
    let main = DataStore::initialize_empty(&path)?;
    assert!(main.auto_tag_settings()?.enabled("photo")?);
    main.set_auto_tags_enabled(&["photo"], false)?;
    let derived = dir.path().join("derived.redb");
    drop(EmbeddingStore::open(&derived)?);
    std::fs::remove_file(derived)?;
    drop(main);
    let main = DataStore::open(&path)?;
    assert!(!main.auto_tag_settings()?.enabled("photo")?);
    main.set_auto_tags_enabled(&["photo"], true)?;
    assert!(main.auto_tag_settings()?.enabled("photo")?);
    // An old reader still sees the frozen, unchanged records table.
    assert_eq!(main.read(|r| Ok::<_, anyhow::Error>(r.iter()?.count()))?, 0);
    Ok(())
}

#[test]
fn rejection_survives_recompute_vocabulary_model_changes_and_derived_rebuild() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let main_path = dir.path().join("main.redb");
    let main = DataStore::initialize_empty(&main_path)?;
    main.write(|writer| writer.reject_auto_tag("photo", "cat"))?;
    let mut input = HashMap::from([(
        "photo".into(),
        Input {
            version: 1,
            tags: vec![],
            enabled: true,
            rejections: main.auto_tag_rejections_for("photo")?,
        },
    )]);
    drop(main);
    let path = dir.path().join("derived.redb");
    let mut original = None;
    for round in 0..3 {
        let main = DataStore::open(&main_path)?;
        input.get_mut("photo").unwrap().rejections = main.auto_tag_rejections_for("photo")?;
        let store = EmbeddingStore::open(&path)?;
        let mut tags = AutoTags::new(store.clone())?;
        tags.catalog.version = format!("{}-{round}", tags.catalog.version);
        tags.catalog.model_id = format!("{}-{round}", tags.catalog.model_id);
        let model = ModelInfo {
            model_id: tags.catalog.model_id.clone(),
            embedding_dim: 2,
            device: "CPU".into(),
            logit_scale: Some(10.0),
            logit_bias: Some(0.0),
        };
        store.set_model(&model)?;
        store.put_concept_vectors(
            &model.model_id,
            &tags.catalog.version,
            &crate::storage::auto_tags::ConceptVectors {
                scale: 10.0,
                bias: 0.0,
                vectors: tags
                    .catalog
                    .concepts
                    .iter()
                    .map(|c| (c.id.clone(), vec![1.0, 0.0]))
                    .collect(),
            },
        )?;
        store.put(
            "photo",
            &model.model_id,
            0,
            &StoredEmbedding {
                thumbnail_version: 1,
                vector: vec![1.0, 0.0],
            },
        )?;
        let fake = Fake(AtomicUsize::new(0));
        tags.prepare(&model, &fake)?;
        for _ in 0..2 {
            tags.materialize(&input, &[])?;
        }
        let result = tags.status("photo", 1, true)?.auto_tags;
        assert!(!result.is_empty());
        assert!(!result.iter().any(|t| t.concept == "cat"));
        if let Some(previous) = &original {
            assert_eq!(&result, previous);
        }
        original = Some(result);
        assert_eq!(fake.0.load(Ordering::Relaxed), 0);
        drop(tags);
        drop(store);
        // Only a disposable test file is removed; authored data stays intact.
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[test]
fn slow_text_preparation_never_blocks_decision_rematerialization() -> Result<()> {
    use std::{
        sync::{Arc, mpsc},
        time::{Duration, Instant},
    };
    struct Slow {
        started: mpsc::SyncSender<()>,
        release: Mutex<Option<mpsc::Receiver<()>>>,
    }
    impl Embedder for Slow {
        fn info(&self) -> Result<ModelInfo> {
            unreachable!()
        }
        fn embed_image(&self, _: Vec<u8>) -> Result<ImageEmbedding> {
            panic!("no image inference")
        }
        fn embed_text(&self, _: &str) -> Result<ImageEmbedding> {
            if let Some(release) = self.release.lock().unwrap().take() {
                self.started.send(())?;
                let _ = release.recv_timeout(Duration::from_secs(2));
            }
            Ok(ImageEmbedding {
                model_id: BuiltinCatalog::embedded()?.model_id,
                embedding: vec![1.0, 0.0],
            })
        }
    }
    let dir = tempfile::tempdir()?;
    let store = EmbeddingStore::open(&dir.path().join("derived.redb"))?;
    let tags = Arc::new(AutoTags::new(store)?);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let worker_tags = tags.clone();
    let worker = std::thread::spawn(move || {
        worker_tags.prepare(
            &ModelInfo {
                model_id: worker_tags.catalog.model_id.clone(),
                embedding_dim: 2,
                device: "CPU".into(),
                logit_scale: Some(10.0),
                logit_bias: Some(0.0),
            },
            &Slow {
                started: started_tx,
                release: Mutex::new(Some(release_rx)),
            },
        )
    });
    started_rx.recv_timeout(Duration::from_secs(2))?;
    let start = Instant::now();
    tags.materialize(&HashMap::new(), &[])?;
    let elapsed = start.elapsed();
    let _ = release_tx.send(());
    worker.join().unwrap()?;
    ensure!(
        elapsed < Duration::from_millis(250),
        "decision waited for text inference: {elapsed:?}"
    );
    Ok(())
}

fn learning_fixture(
    count: usize,
    dim: usize,
) -> Result<(tempfile::TempDir, AutoTags, PhotoInputs, ModelInfo)> {
    let dir = tempfile::tempdir()?;
    let store = EmbeddingStore::open(&dir.path().join("derived.redb"))?;
    let tags = AutoTags::new(store.clone())?;
    let model = ModelInfo {
        model_id: tags.catalog.model_id.clone(),
        embedding_dim: dim,
        device: "CPU".into(),
        logit_scale: Some(10.0),
        logit_bias: Some(-5.0),
    };
    store.set_model(&model)?;
    let mut vector = vec![0.0; dim];
    vector[dim - 1] = 1.0;
    store.put_concept_vectors(
        &model.model_id,
        &tags.catalog.version,
        &ConceptVectors {
            scale: 10.0,
            bias: -5.0,
            vectors: tags
                .catalog
                .concepts
                .iter()
                .map(|c| (c.id.clone(), vector.clone()))
                .collect(),
        },
    )?;
    tags.prepare(&model, &Fake(AtomicUsize::new(0)))?;
    let mut inputs = HashMap::new();
    for i in 0..count {
        let id = format!("{i:064x}");
        let mut vector = vec![0.0; dim];
        vector[usize::from((5..10).contains(&i))] = 1.0;
        store.put(
            &id,
            &model.model_id,
            0,
            &StoredEmbedding {
                thumbnail_version: 1,
                vector,
            },
        )?;
        inputs.insert(
            id,
            Input {
                version: 1,
                tags: if i < 4 { vec!["Moka".into()] } else { vec![] },
                enabled: true,
                rejections: vec![],
            },
        );
    }
    Ok((dir, tags, inputs, model))
}

#[test]
fn feedback_retrains_all_photos_and_rebuilds_identical_derived_predictions() -> Result<()> {
    use crate::operations::autotag::learning::personal_id;
    let (dir, tags, mut inputs, model) = learning_fixture(12, 3)?;
    let target = format!("{:064x}", 10);
    let concept = personal_id("Moka");
    let fires = |tags: &AutoTags| -> Result<bool> {
        Ok(tags
            .status(&target, 1, true)?
            .auto_tags
            .iter()
            .any(|t| t.concept == concept))
    };
    tags.materialize(&inputs, &[])?;
    assert!(!fires(&tags)?);
    inputs
        .get_mut(&format!("{:064x}", 4))
        .unwrap()
        .tags
        .push("Moka".into());
    // A newly authored tag invalidates every photo, not just the edited one.
    tags.materialize(&inputs, &[])?;
    assert!(fires(&tags)?);
    for i in 5..10 {
        inputs
            .get_mut(&format!("{i:064x}"))
            .unwrap()
            .rejections
            .push(concept.clone());
    }
    tags.materialize(&inputs, &[])?;
    assert!(fires(&tags)?);
    inputs
        .get_mut(&target)
        .unwrap()
        .rejections
        .push(concept.clone());
    tags.materialize(&inputs, &[])?;
    assert!(!fires(&tags)?);
    let weights = tags
        .store
        .learning(&model.model_id, &tags.catalog.version)?;
    assert!(weights.as_ref().unwrap().classifiers.contains_key(&concept));
    let original = tags
        .store
        .all_auto_tags()?
        .into_iter()
        .map(|(id, row)| (id, row.tags))
        .collect::<std::collections::BTreeMap<_, _>>();
    let vectors = tags
        .store
        .concept_vectors(&model.model_id, &tags.catalog.version)?
        .unwrap();
    let embeddings = tags.store.embeddings(&model.model_id)?;
    let vocabulary = tags.catalog.version.clone();
    drop(tags);
    // Delete only this test's derived file. Re-embedding supplies the same model vectors;
    // no saved coefficients, prototypes or materialized tags survive this step.
    let path = dir.path().join("derived.redb");
    std::fs::remove_file(&path)?;
    let store = EmbeddingStore::open(&path)?;
    store.set_model(&model)?;
    store.put_concept_vectors(&model.model_id, &vocabulary, &vectors)?;
    for (id, embedding) in embeddings {
        store.put(&id, &model.model_id, 0, &embedding)?;
    }
    let rebuilt = AutoTags::new(store)?;
    rebuilt.prepare(&model, &Fake(AtomicUsize::new(0)))?;
    rebuilt.materialize(&inputs, &[])?;
    assert_eq!(
        rebuilt
            .store
            .learning(&model.model_id, &rebuilt.catalog.version)?,
        weights
    );
    let results = rebuilt
        .store
        .all_auto_tags()?
        .into_iter()
        .map(|(id, row)| (id, row.tags))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(results, original);
    // A restart also retrains rather than trusting persisted coefficients.
    let restarted = AutoTags::new(rebuilt.store.clone())?;
    restarted.prepare(&model, &Fake(AtomicUsize::new(0)))?;
    restarted.materialize(&inputs, &[])?;
    assert!(!fires(&restarted)?);
    // Removing one of the five positives closes the personal gate again.
    inputs.get_mut(&format!("{:064x}", 0)).unwrap().tags.clear();
    rebuilt.materialize(&inputs, &[])?;
    assert!(!fires(&rebuilt)?);
    assert!(
        rebuilt
            .status(&format!("{:064x}", 11), 1, true)?
            .auto_tags
            .iter()
            .all(|t| t.concept != concept)
    );
    assert!(
        rebuilt
            .store
            .learning(&model.model_id, &rebuilt.catalog.version)?
            .unwrap()
            .classifiers
            .is_empty()
    );
    Ok(())
}

#[test]
#[ignore = "explicit local release-profile performance check"]
fn learning_recompute_benchmark() -> Result<()> {
    use std::time::{Duration, Instant};
    let (_dir, tags, mut inputs, _) = learning_fixture(7_100, 768)?;
    tags.materialize(&inputs, &[])?;
    inputs
        .get_mut(&format!("{:064x}", 4))
        .unwrap()
        .tags
        .push("Moka".into());
    let concept = crate::operations::autotag::learning::personal_id("Moka");
    for i in 5..10 {
        inputs
            .get_mut(&format!("{i:064x}"))
            .unwrap()
            .rejections
            .push(concept.clone());
    }
    let start = Instant::now();
    tags.materialize(&inputs, &[])?;
    let elapsed = start.elapsed();
    eprintln!(
        "Learning + materialization: 7100 photos x 768 dimensions, {} built-ins + 1 personal, {elapsed:?}",
        tags.catalog.concepts.len()
    );
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    assert!(
        tags.status(&format!("{:064x}", 10), 1, true)?
            .auto_tags
            .iter()
            .any(|t| t.concept == concept)
    );
    Ok(())
}
