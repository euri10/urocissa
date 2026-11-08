use super::*;
use crate::{
    operations::{auto_tags::AutoTags, autotag::BuiltinCatalog, embedding},
    storage::{
        auto_tags::{MaterializedTags, StoredAutoTag},
        embeddings::EmbeddingStore,
    },
};
use serde_json::json;

pub(super) async fn checks(client: &Client) {
    embedding::install_test_service();
    let store = EmbeddingStore::open(
        &crate::public::constant::storage::get_data_path().join("db/embeddings.redb"),
    )
    .unwrap();
    let catalog = BuiltinCatalog::embedded().unwrap();
    store
        .set_model(&embedding::client::ModelInfo {
            model_id: catalog.model_id.clone(),
            embedding_dim: 2,
            device: "CPU".into(),
            logit_scale: Some(10.0),
            logit_bias: Some(0.0),
        })
        .unwrap();
    store
        .put_auto_tags(&[(
            PUBLIC.into(),
            MaterializedTags {
                enabled: true,
                model_id: catalog.model_id,
                vocabulary_version: catalog.version,
                thumbnail_version: 7,
                tags: vec![StoredAutoTag {
                    concept: "cat".into(),
                    label: "chat".into(),
                    confidence: 0.91,
                    displayed: true,
                }],
            },
        )])
        .unwrap();
    assert!(
        embedding::service()
            .unwrap()
            .auto_tags
            .set(AutoTags::new(store.clone()).unwrap())
            .is_ok()
    );

    let response = owner(client.get(format!("/get/resource/{PUBLIC}")), true)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let record = response.into_json::<Value>().await.unwrap();
    assert_eq!(
        record["data"]["abstractData"]["autoTags"][0]["label"],
        "chat"
    );
    assert_eq!(record["data"]["abstractData"]["autoTagsEnabled"], true);
    assert!(
        !record["data"]["abstractData"]["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("chat"))
    );
    let owner_snapshot = snapshot(client, true, "descending").await;
    let response = owner(client.get(snapshot_path("data", &owner_snapshot)), true)
        .dispatch()
        .await;
    let rows = response.into_json::<Vec<Value>>().await.unwrap();
    assert!(
        rows.iter()
            .any(|r| r["abstractData"]["autoTags"][0]["label"] == "chat")
    );

    for request in [
        client.get(format!("/get/auto-tags/{PUBLIC}")),
        client
            .get(format!("/get/auto-tags/{PUBLIC}"))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    ] {
        assert_eq!(request.dispatch().await.status(), Status::Unauthorized);
    }
    // Both direct records and batches apply the owner gate, not just the new endpoint.
    let shared = client
        .post("/get/prefetch")
        .header(ContentType::JSON)
        .header(Header::new("x-album-id", ALBUM))
        .header(Header::new("x-share-id", "fixture-share"))
        .dispatch()
        .await
        .into_json::<PrefetchReturn>()
        .await
        .unwrap();
    for request in [
        client
            .get(format!("/get/resource/{PUBLIC}"))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
        client
            .get(snapshot_path("data", &shared))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    ] {
        let response = request.dispatch().await;
        assert_eq!(response.status(), Status::Ok);
        assert!(!response.into_string().await.unwrap().contains("autoTags"));
    }
    if crate::router::fairing::guard_gallery::public_gallery_enabled() {
        for path in [
            format!("/get/resource/{PUBLIC}"),
            snapshot_path("data", &snapshot(client, false, "descending").await),
        ] {
            let response = client.get(path).dispatch().await;
            assert_eq!(response.status(), Status::Ok);
            assert!(!response.into_string().await.unwrap().contains("autoTags"));
        }
    }
    let body = json!({"hash": PUBLIC, "enabled": false});
    assert_eq!(
        client
            .put("/put/edit_auto_tags")
            .json(&body)
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
        owner(client.put("/put/edit_auto_tags").json(&body), true)
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
    assert_eq!(
        owner(client.put("/put/edit_auto_tags").json(&body), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    let response = owner(client.get(format!("/get/auto-tags/{PUBLIC}")), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert_eq!(response["autoTagsEnabled"], false);
    assert_eq!(response["autoTags"], json!([]));
    for body in [
        json!({"hash": "bad", "enabled": true}),
        json!({"hash": ALBUM, "enabled": true}),
        json!({"hash": PUBLIC, "timestamp": 1, "enabled": true}),
    ] {
        assert_eq!(
            owner(client.put("/put/edit_auto_tags").json(&body), true)
                .dispatch()
                .await
                .status(),
            Status::BadRequest
        );
    }
    // Videos are ignored, and compact snapshot selections use the existing resolver.
    assert_eq!(
        owner(
            client
                .put("/put/edit_auto_tags")
                .json(&json!({"hash": PUBLIC_VIDEO, "enabled": false})),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert!(
        TREE.store
            .auto_tag_settings()
            .unwrap()
            .enabled(PUBLIC_VIDEO)
            .unwrap()
    );
    let index = rows
        .iter()
        .position(|r| r["abstractData"]["id"] == PUBLIC)
        .unwrap();
    let response = owner(client.put("/put/edit_auto_tags").json(&json!({"timestamp": owner_snapshot.prefetch.timestamp, "selection": {"mode": "explicit", "indices": [index]}, "enabled": true})), true).dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    TREE.store.set_auto_tags_enabled(&[PUBLIC], true).unwrap();
    Box::pin(super::tag_search::checks(client, &store)).await;
}
