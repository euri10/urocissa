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
