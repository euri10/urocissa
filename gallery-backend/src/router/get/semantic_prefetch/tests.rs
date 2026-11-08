use crate::{
    operations::{
        embedding::{
            self,
            client::{Embedder, ImageEmbedding, ModelInfo},
        },
        semantic::SemanticSearch,
    },
    public::{
        constant::storage::DATA_PATH,
        db::{
            tree::{TREE, state::TreeState},
            tree_snapshot::TREE_SNAPSHOT,
        },
        structure::{
            abstract_data::AbstractData,
            album::{ResolvedShare, Share},
            config::{APP_CONFIG, AppConfig},
            image::{ImageCombined, ImageMetadata},
            object::{ObjectSchema, ObjectType},
        },
    },
    router::{
        claims::claims::Claims,
        get::get_prefetch::{PrefetchReturn, prefetch},
    },
    storage::{
        DataStore,
        embeddings::{EmbeddingStore, StoredEmbedding},
    },
};
use anyhow::Result;
use arrayvec::ArrayString;
use rocket::{
    http::{ContentType, Cookie, Status},
    local::blocking::Client,
};
use std::{
    process::Command,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Fake(AtomicBool, AtomicUsize);

#[path = "similar_tests.rs"]
mod similar;
impl Embedder for Fake {
    fn info(&self) -> Result<ModelInfo> {
        anyhow::ensure!(self.0.load(Ordering::Relaxed), "offline");
        Ok(ModelInfo {
            model_id: "test-model".into(),
            embedding_dim: self.1.load(Ordering::Relaxed),
            device: "CPU".into(),
            logit_scale: None,
            logit_bias: None,
        })
    }
    fn embed_image(&self, _: Vec<u8>) -> Result<ImageEmbedding> {
        anyhow::bail!("unused")
    }
    fn embed_text(&self, _: &str) -> Result<ImageEmbedding> {
        let mut embedding = vec![0.0; self.1.load(Ordering::Relaxed)];
        embedding[0] = 1.0;
        Ok(ImageEmbedding {
            model_id: "test-model".into(),
            embedding,
        })
    }
}

fn photo(number: u32, version: u32) -> AbstractData {
    let id = ArrayString::from(&format!("{number:064x}")).unwrap();
    let mut object = ObjectSchema::new(id, ObjectType::Image);
    object.cache_version = version;
    AbstractData::Image(ImageCombined {
        object,
        metadata: ImageMetadata::new(id, 10, 32, 24, "jpg".into()),
    })
}

fn ids(result: &PrefetchReturn) -> Vec<String> {
    let snapshot = TREE_SNAPSHOT
        .read_tree_snapshot(result.prefetch.timestamp)
        .unwrap();
    (0..snapshot.len())
        .map(|index| snapshot.get_hash(index).unwrap().to_string())
        .collect()
}

#[test]
fn semantic_http_search_gate() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_SEMANTIC_TEST", "1")
        .env("UROCISSA_PUBLIC_GALLERY", "1")
        .env("ROCKET_LOG_LEVEL", "off")
        .args([
            "--exact",
            "router::get::semantic_prefetch::tests::http_child",
            "--nocapture",
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("semantic HTTP gate timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn http_child() {
    if std::env::var_os("UROCISSA_SEMANTIC_TEST").is_none() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("db")).unwrap();
    DATA_PATH.set(root.path().to_owned()).unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("fixture-password".into());
    config.private.auth_key = Some("semantic-fixture-key".into());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    let records = vec![photo(1, 0), photo(2, 0), photo(3, 7), photo(4, 0)];
    let store = DataStore::initialize_empty(&root.path().join("db/index_v6.redb")).unwrap();
    store
        .write(|writer| -> Result<()> {
            for record in &records {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    // Deliberately leave all photos private; owners may semantically search them.
    store.migrate_photo_audience().unwrap();
    store.migrate_album_audience().unwrap();
    for n in 1..=4 {
        store
            .set_photo_audience(
                &format!("{n:064x}"),
                crate::public::structure::audience::Audience::Private,
            )
            .unwrap();
    }
    drop(store);
    *TREE.state.write().unwrap() = TreeState::from_records(records);
    let cache = EmbeddingStore::open(&root.path().join("db/embeddings.redb")).unwrap();
    for (n, version, vector) in [
        (1, 0, vec![1.0, 0.0]),
        (2, 0, vec![0.95, (1.0_f32 - 0.95 * 0.95).sqrt()]),
        (3, 0, vec![1.0, 0.0]),
        (99, 0, vec![1.0, 0.0]),
    ] {
        cache
            .put(
                &format!("{n:064x}"),
                "test-model",
                0,
                &StoredEmbedding {
                    thumbnail_version: version,
                    vector,
                },
            )
            .unwrap();
    }
    let fake = Arc::new(Fake(AtomicBool::new(true), AtomicUsize::new(2)));
    cache.set_model(&fake.info().unwrap()).unwrap();
    embedding::install_test_service();
    assert!(
        embedding::service()
            .unwrap()
            .search
            .set(SemanticSearch::new(fake.clone(), cache.clone()))
            .is_ok()
    );
    let client = Client::tracked(rocket::build().mount("/", routes![prefetch])).unwrap();
    let body = r#"{"And":[{"Semantic":"enfant à vélo"},{"Trashed":false},{"Audience":"private"}]}"#;
    let request = |query: &str, body: &str| {
        client
            .post(format!("/get/prefetch?{query}"))
            .header(ContentType::JSON)
            .body(body)
    };
    let owner = || Cookie::new("jwt", Claims::new_admin().encode());
    similar::check(&client, &fake);
    let response = request("sort=relevance", body).cookie(owner()).dispatch();
    assert_eq!(response.status(), Status::Ok);
    let first: PrefetchReturn = response.into_json().unwrap();
    assert_eq!(first.prefetch.data_length, 2);
    let summary = first.semantic_search.as_ref().unwrap();
    assert_eq!((summary.indexed_photos, summary.pending_photos), (2, 2));
    for n in [3, 4] {
        cache
            .put(
                &format!("{n:064x}"),
                "test-model",
                0,
                &StoredEmbedding {
                    thumbnail_version: if n == 3 { 7 } else { 0 },
                    vector: vec![1.0, 0.0],
                },
            )
            .unwrap();
    }
    let mut expected = ids(&first);
    expected.sort();
    for sort in ["ascending", "descending", "relevance", "random"] {
        let response = request(
            &format!("sort={sort}&semantic_selection={}", summary.selection),
            body,
        )
        .cookie(owner())
        .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let result: PrefetchReturn = response.into_json().unwrap();
        let mut actual = ids(&result);
        actual.sort();
        assert_eq!(actual, expected, "sorting changed membership");
    }
    let fresh: PrefetchReturn = request("sort=relevance", body)
        .cookie(owner())
        .dispatch()
        .into_json()
        .unwrap();
    assert_eq!(fresh.prefetch.data_length, 4);
    assert_eq!(
        request(
            &format!("semantic_selection={}", summary.selection),
            r#"{"Semantic":"different"}"#
        )
        .cookie(owner())
        .dispatch()
        .status(),
        Status::Conflict
    );
    for expression in [
        r#"{"Semantic":"bird"}"#,
        r#"{"Not":{"Semantic":"bird"}}"#,
        r#"{"Or":[{"Semantic":"bird"},{"Trashed":false}]}"#,
    ] {
        assert_eq!(
            request("sort=relevance", expression).dispatch().status(),
            Status::Forbidden
        );
        let share = Claims {
            role: crate::router::claims::Role::Share(Box::new(ResolvedShare {
                album_id: ArrayString::from(&"a".repeat(64)).unwrap(),
                album_title: None,
                share: Share::default(),
            })),
            ..Claims::new_admin()
        }
        .encode();
        assert_eq!(
            request("sort=relevance", expression)
                .cookie(Cookie::new("jwt", share))
                .dispatch()
                .status(),
            Status::Forbidden
        );
    }
    for invalid in [r#"{"Not":{"Semantic":"bird"}}"#, r#"{"Semantic":" "}"#] {
        assert_eq!(
            request("sort=relevance", invalid)
                .cookie(owner())
                .dispatch()
                .status(),
            Status::BadRequest
        );
    }
    assert_eq!(
        request("sort=relevance", "null")
            .cookie(owner())
            .dispatch()
            .status(),
        Status::BadRequest
    );
    assert_eq!(
        request(
            "sort=relevance",
            r#"{"And":[{"Semantic":"bird"},{"Tag":"missing"}]}"#
        )
        .cookie(owner())
        .dispatch()
        .into_json::<PrefetchReturn>()
        .unwrap()
        .prefetch
        .data_length,
        0
    );
    fake.0.store(false, Ordering::Relaxed);
    assert_eq!(
        request("sort=relevance", body)
            .cookie(owner())
            .dispatch()
            .status(),
        Status::ServiceUnavailable
    );
    let normal = request("sort=descending", r#"{"Trashed":false}"#)
        .cookie(owner())
        .dispatch();
    assert_eq!(normal.status(), Status::Ok);
    assert!(!normal.into_string().unwrap().contains("semanticSearch"));
    similar::ordering_and_selection(&client, &cache);
    benchmark_full_request(&client, &cache, &fake);
}

fn benchmark_full_request(client: &Client, cache: &EmbeddingStore, fake: &Fake) {
    fake.0.store(true, Ordering::Relaxed);
    fake.1.store(768, Ordering::Relaxed);
    cache.set_model(&fake.info().unwrap()).unwrap();
    let vector = vec![1.0 / 768.0_f32.sqrt(); 768];
    for number in 1..=10_000 {
        cache
            .put(
                &format!("{number:064x}"),
                "test-model",
                0,
                &StoredEmbedding {
                    thumbnail_version: 0,
                    vector: vector.clone(),
                },
            )
            .unwrap();
    }
    *TREE.state.write().unwrap() =
        TreeState::from_records((1..=10_000).map(|number| photo(number, 0)));
    for body in [
        r#"{"Semantic":"ten thousand photo benchmark"}"#.to_owned(),
        format!(r#"{{"Similar":"{:064x}"}}"#, 1),
    ] {
        let start = Instant::now();
        let response = client
            .post("/get/prefetch?sort=relevance")
            .header(ContentType::JSON)
            .cookie(Cookie::new("jwt", Claims::new_admin().encode()))
            .body(body.clone())
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        let result: PrefetchReturn = response.into_json().unwrap();
        let elapsed = start.elapsed();
        eprintln!("10k × 768 complete ranked request ({body}): {elapsed:?}");
        assert_eq!(result.prefetch.data_length, 200);
        assert_eq!(
            result.semantic_search.as_ref().unwrap().indexed_photos,
            if body.contains("Similar") {
                9_999
            } else {
                10_000
            }
        );
        if body.contains("Similar") {
            assert!(!ids(&result).contains(&format!("{:064x}", 1)));
        }
        assert!(
            elapsed < Duration::from_millis(if cfg!(debug_assertions) { 1000 } else { 200 }),
            "full request budget exceeded: {elapsed:?}"
        );
    }
}
