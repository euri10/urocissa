use super::*;
use crate::public::structure::expression::{FilterValue, TagSource};
use serde_json::json;

async fn decision(client: &Client, hash: &str, concept: &str, action: &str) -> Value {
    let response = owner(
        client.put("/put/auto_tag_decision").json(&json!({
            "hash": hash, "concept": concept, "action": action
        })),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

fn stale_prediction(hash: &str, concept: &str) {
    use crate::{
        operations::autotag::BuiltinCatalog,
        storage::auto_tags::{MaterializedTags, StoredAutoTag},
    };
    let store = crate::operations::embedding::service()
        .unwrap()
        .auto_tags
        .get()
        .unwrap()
        .test_store();
    let catalog = BuiltinCatalog::embedded().unwrap();
    store
        .put_auto_tags(&[(
            hash.into(),
            MaterializedTags {
                enabled: true,
                model_id: catalog.model_id,
                vocabulary_version: catalog.version,
                thumbnail_version: 7,
                tags: vec![StoredAutoTag {
                    concept: concept.into(),
                    label: concept.into(),
                    confidence: 0.99,
                    displayed: true,
                }],
            },
        )])
        .unwrap();
}

async fn source_count(client: &Client, concept: &str, source: TagSource) -> usize {
    let response = owner(
        client.post("/get/prefetch").json(&Expression::And(vec![
            Expression::Tag(FilterValue::Value(concept.into())),
            Expression::Source(source),
        ])),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response
        .into_json::<PrefetchReturn>()
        .await
        .unwrap()
        .prefetch
        .data_length
}

#[allow(
    clippy::too_many_lines,
    reason = "Sequential owner/visitor HTTP acceptance scenario"
)]
pub(super) async fn checks(client: &Client) {
    for action in ["confirm", "reject"] {
        let body = json!({"hash": PUBLIC, "concept": "cat", "action": action});
        for request in [
            client.put("/put/auto_tag_decision").json(&body),
            client
                .put("/put/auto_tag_decision")
                .json(&body)
                .header(Header::new("x-album-id", ALBUM))
                .header(Header::new("x-share-id", "fixture-share")),
        ] {
            assert_eq!(request.dispatch().await.status(), Status::Unauthorized);
        }
    }
    assert_eq!(
        client
            .post("/post/auto_tag_selection")
            .json(&json!({"hash": PUBLIC}))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    APP_CONFIG
        .get()
        .unwrap()
        .write()
        .unwrap()
        .public
        .read_only_mode = true;
    assert_eq!(
        owner(
            client
                .put("/put/auto_tag_decision")
                .json(&json!({"hash": PUBLIC, "concept": "cat", "action": "confirm"})),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::MethodNotAllowed
    );
    APP_CONFIG
        .get()
        .unwrap()
        .write()
        .unwrap()
        .public
        .read_only_mode = false;
    for body in [
        json!({"hash": ALBUM, "concept": "cat", "action": "confirm"}),
        json!({"hash": "invalid", "concept": "cat", "action": "reject"}),
        json!({"hash": PUBLIC, "concept": "unknown", "action": "confirm"}),
        json!({"hash": PUBLIC, "timestamp": 1, "concept": "cat", "action": "confirm"}),
    ] {
        assert_eq!(
            owner(client.put("/put/auto_tag_decision").json(&body), true)
                .dispatch()
                .await
                .status(),
            Status::BadRequest
        );
    }
    // Confirmation is durable ordinary metadata, independent of the feature and worker.
    stale_prediction(PUBLIC, "cat");
    assert_eq!(
        decision(client, PUBLIC, "cat", "confirm").await,
        json!({"photos": 1, "label": "cat"})
    );
    assert_eq!(
        decision(client, PUBLIC, "cat", "confirm").await["photos"],
        1
    );
    assert!(
        TREE.store
            .reader()
            .unwrap()
            .get(PUBLIC)
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains("cat")
    );
    assert_eq!(source_count(client, "cat", TagSource::User).await, 1);
    assert_eq!(source_count(client, "cat", TagSource::Auto).await, 0);
    for request in [
        owner(client.get(format!("/get/resource/{PUBLIC}")), true),
        client
            .get(format!("/get/resource/{PUBLIC}"))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    ] {
        let data = request.dispatch().await.into_json::<Value>().await.unwrap();
        assert!(
            data["data"]["abstractData"]["tags"]
                .as_array()
                .unwrap()
                .contains(&json!("cat"))
        );
    }
    if crate::router::fairing::guard_gallery::public_gallery_enabled() {
        let data = client
            .get(format!("/get/resource/{PUBLIC}"))
            .dispatch()
            .await
            .into_json::<Value>()
            .await
            .unwrap();
        assert!(
            data["data"]["abstractData"]["tags"]
                .as_array()
                .unwrap()
                .contains(&json!("cat"))
        );
    }
    TREE.store.set_auto_tags_enabled(&[PUBLIC], false).unwrap();
    assert!(
        TREE.store
            .reader()
            .unwrap()
            .get(PUBLIC)
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains("cat")
    );
    let snapshot = snapshot(client, true, "descending").await;
    let rows = owner(client.get(snapshot_path("data", &snapshot)), true)
        .dispatch()
        .await
        .into_json::<Vec<Value>>()
        .await
        .unwrap();
    let index = rows
        .iter()
        .position(|r| r["abstractData"]["id"] == PUBLIC)
        .unwrap();
    assert_eq!(owner(client.put("/put/edit_tag").json(&json!({
        "timestamp": snapshot.prefetch.timestamp, "selection": {"mode": "explicit", "indices": [index]}, "addTagsArray": [], "removeTagsArray": ["cat"]
    })), true).dispatch().await.status(), Status::Ok);
    assert!(
        !TREE
            .store
            .reader()
            .unwrap()
            .get(PUBLIC)
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains("cat")
    );
    assert!(
        TREE.store
            .auto_tag_rejections_for(PUBLIC)
            .unwrap()
            .contains(&"cat".to_owned())
    );
    TREE.store.set_auto_tags_enabled(&[PUBLIC], true).unwrap();
    assert_eq!(source_count(client, "cat", TagSource::Auto).await, 0);
    stale_prediction(PRIVATE, "bird");
    decision(client, PRIVATE, "bird", "reject").await;
    decision(client, PRIVATE, "bird", "reject").await;
    assert_eq!(
        TREE.store.auto_tag_rejections_for(PRIVATE).unwrap(),
        vec!["bird"]
    );
    assert_eq!(source_count(client, "bird", TagSource::Auto).await, 0);
    let facets = owner(client.get("/get/get-search-facets"), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert!(
        !facets["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tag| tag["value"] == "cat" || tag["value"] == "bird")
    );
    let response = owner(client.get(format!("/get/auto-tags/{PUBLIC}")), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert!(
        !response["autoTags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tag| tag["concept"] == "cat")
    );
    assert_eq!(
        decision(client, PUBLIC_VIDEO, "cat", "confirm").await["photos"],
        0
    );
    bulk(client).await;
}

#[allow(
    clippy::too_many_lines,
    reason = "Fixture setup and timed 500-photo HTTP acceptance scenario"
)]
async fn bulk(client: &Client) {
    use crate::{
        operations::{
            auto_tags::AutoTags,
            autotag::BuiltinCatalog,
            embedding::client::{Embedder, ImageEmbedding, ModelInfo},
        },
        storage::{auto_tags::ConceptVectors, embeddings::StoredEmbedding},
    };
    struct CachedOnly;
    impl Embedder for CachedOnly {
        fn info(&self) -> anyhow::Result<ModelInfo> {
            unreachable!()
        }
        fn embed_image(&self, _: Vec<u8>) -> anyhow::Result<ImageEmbedding> {
            panic!("no image inference")
        }
        fn embed_text(&self, _: &str) -> anyhow::Result<ImageEmbedding> {
            panic!("no text inference")
        }
    }
    const DIM: usize = 768;
    let vector = vec![1.0 / 768.0_f32.sqrt(); DIM];
    let records: Vec<_> = (0..500)
        .map(|i| {
            let hash = format!("{i:064x}");
            let id = id(&hash);
            AbstractData::Image(ImageCombined {
                object: ObjectSchema::new(id, ObjectType::Image),
                metadata: ImageMetadata::new(id, 13, 32, 24, "jpg".into()),
            })
        })
        .collect();
    TREE.store
        .write(|writer| {
            for record in &records {
                writer.insert(record)?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .unwrap();
    *TREE.state.write().unwrap() = TreeState::from_records(records.clone());
    let catalog = BuiltinCatalog::embedded().unwrap();
    let tags: &AutoTags = crate::operations::embedding::service()
        .unwrap()
        .auto_tags
        .get()
        .unwrap();
    let store = tags.test_store();
    let model = ModelInfo {
        model_id: catalog.model_id.clone(),
        embedding_dim: DIM,
        device: "CPU".into(),
        logit_scale: Some(10.0),
        logit_bias: Some(0.0),
    };
    store.set_model(&model).unwrap();
    store
        .put_concept_vectors(
            &model.model_id,
            &catalog.version,
            &ConceptVectors {
                scale: 10.0,
                bias: 0.0,
                vectors: catalog
                    .concepts
                    .iter()
                    .map(|c| (c.id.clone(), vector.clone()))
                    .collect(),
            },
        )
        .unwrap();
    for record in &records {
        store
            .put(
                record.hash().as_str(),
                &model.model_id,
                0,
                &StoredEmbedding {
                    thumbnail_version: 0,
                    vector: vector.clone(),
                },
            )
            .unwrap();
    }
    tags.prepare(&model, &CachedOnly).unwrap();
    tags.reconcile().unwrap();
    assert!(
        store
            .auto_tags(records[0].hash().as_str())
            .unwrap()
            .unwrap()
            .tags
            .iter()
            .any(|t| t.concept == "cat")
    );
    let snapshot = snapshot(client, true, "descending").await;
    let selection = json!({"timestamp": snapshot.prefetch.timestamp, "selection": {"mode": "allExcept", "excludedIndices": []}});
    let preview = owner(
        client.post("/post/auto_tag_selection").json(&selection),
        true,
    )
    .dispatch()
    .await
    .into_json::<Value>()
    .await
    .unwrap();
    assert_eq!(preview["photos"], 500);
    assert_eq!(preview["publicPhotos"], 0);
    for (concept, action) in [("cat", "confirm"), ("dog", "reject")] {
        let mut body = selection.clone();
        body["concept"] = json!(concept);
        body["action"] = json!(action);
        let started = Instant::now();
        let response = owner(client.put("/put/auto_tag_decision").json(&body), true)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.into_json::<Value>().await.unwrap()["photos"], 500);
        eprintln!(
            "500-photo {action} + rematerialization: {:?}",
            started.elapsed()
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        for record in &records {
            let row = store.auto_tags(record.hash().as_str()).unwrap().unwrap();
            assert!(!row.tags.iter().any(|tag| tag.concept == concept));
            if action == "confirm" {
                assert!(
                    TREE.store
                        .reader()
                        .unwrap()
                        .get(record.hash().as_str())
                        .unwrap()
                        .unwrap()
                        .into_value()
                        .tag()
                        .contains("cat")
                );
            } else {
                assert!(
                    TREE.store
                        .auto_tag_rejections_for(record.hash().as_str())
                        .unwrap()
                        .contains(&concept.to_owned())
                );
            }
        }
    }
    let stale = json!({"timestamp": -1, "selection": {"mode": "explicit", "indices": [0]}, "concept": "cat", "action": "reject"});
    assert_eq!(
        owner(client.put("/put/auto_tag_decision").json(&stale), true)
            .dispatch()
            .await
            .status(),
        Status::Conflict
    );
    personal_learning(client).await;
    super::recalculation::checks(client, &records, &store, &model, &vector).await;
}

async fn personal_learning(client: &Client) {
    let snapshot = snapshot(client, true, "descending").await;
    let rows = owner(client.get(snapshot_path("data", &snapshot)), true)
        .dispatch()
        .await
        .into_json::<Vec<Value>>()
        .await
        .unwrap();
    let target = rows[10]["abstractData"]["id"].as_str().unwrap();
    let confirmed = rows[11]["abstractData"]["id"].as_str().unwrap();
    let concept = crate::operations::autotag::learning::personal_id("Moka");
    let add = |indices: Vec<usize>| {
        json!({
            "timestamp": snapshot.prefetch.timestamp,
            "selection": {"mode": "explicit", "indices": indices},
            "addTagsArray": ["Moka"], "removeTagsArray": [],
        })
    };
    assert_eq!(
        owner(
            client.put("/put/edit_tag").json(&add(vec![0, 1, 2, 3])),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert_eq!(source_count(client, "Moka", TagSource::Auto).await, 0);
    assert_eq!(
        owner(client.put("/put/edit_tag").json(&add(vec![4])), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert_eq!(source_count(client, " MOKA ", TagSource::Auto).await, 495);
    let preview = owner(
        client
            .post("/post/auto_tag_selection")
            .json(&json!({"hash": target})),
        true,
    )
    .dispatch()
    .await
    .into_json::<Value>()
    .await
    .unwrap();
    assert!(
        preview["concepts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["concept"] == concept && c["label"] == "Moka")
    );
    decision(client, target, &concept, "reject").await;
    assert_eq!(source_count(client, "Moka", TagSource::Auto).await, 494);
    assert_eq!(
        decision(client, confirmed, &concept, "confirm").await["label"],
        "Moka"
    );
    assert_eq!(source_count(client, "Moka", TagSource::User).await, 6);
    assert_eq!(source_count(client, "Moka", TagSource::Auto).await, 493);
    let remove = json!({
        "timestamp": snapshot.prefetch.timestamp,
        "selection": {"mode": "explicit", "indices": [11]},
        "addTagsArray": [], "removeTagsArray": ["Moka"],
    });
    assert_eq!(
        owner(client.put("/put/edit_tag").json(&remove), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert!(
        TREE.store
            .auto_tag_rejections_for(confirmed)
            .unwrap()
            .contains(&concept)
    );
    assert_eq!(source_count(client, "Moka", TagSource::User).await, 5);
    assert_eq!(source_count(client, "Moka", TagSource::Auto).await, 493);
}
