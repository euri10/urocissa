//! Real HTTP and derived writes, with the same cached-only embedder as feedback tests.
use super::*;
use crate::{
    operations::embedding::{self, client::ModelInfo},
    storage::embeddings::{EmbeddingStore, StoredEmbedding},
};

async fn status(client: &Client) -> Value {
    owner(client.get("/get/auto-tag-status"), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap()
}

pub(super) async fn boundaries(client: &Client) {
    for path in ["/get/auto-tag-status", "/put/recalculate_auto_tags"] {
        for shared in [false, true] {
            let mut request = if path.starts_with("/get") {
                client.get(path)
            } else {
                client.put(path)
            };
            if shared {
                request = request
                    .header(Header::new("x-album-id", ALBUM))
                    .header(Header::new("x-share-id", "fixture-share"));
            }
            assert_eq!(request.dispatch().await.status(), Status::Unauthorized);
        }
    }
    assert!(status(client).await["lastCompletedAt"].is_null());
    APP_CONFIG
        .get()
        .unwrap()
        .write()
        .unwrap()
        .public
        .read_only_mode = true;
    assert_eq!(
        owner(client.get("/get/auto-tag-status"), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert_eq!(
        owner(client.put("/put/recalculate_auto_tags"), true)
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
}

#[allow(
    clippy::too_many_lines,
    reason = "One real cached-only HTTP recalculation acceptance sequence"
)]
pub(super) async fn checks(
    client: &Client,
    records: &[AbstractData],
    store: &EmbeddingStore,
    model: &ModelInfo,
    vector: &[f32],
) {
    let tags = embedding::service().unwrap().auto_tags.get().unwrap();
    let before = status(client).await;
    assert_eq!(before["upToDate"], true);
    assert!(!before["lastCompletedAt"].is_null());
    // A new authored revision cannot inherit an old run's up-to-date claim.
    crate::public::db::tree::VERSION_COUNT_TIMESTAMP
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(status(client).await["upToDate"], false);
    let rows = store.all_auto_tags().unwrap();
    let response = owner(client.put("/put/recalculate_auto_tags"), true)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let unchanged = response.into_json::<Value>().await.unwrap();
    assert_eq!(
        unchanged["changes"],
        serde_json::json!({"added": 0, "removed": 0})
    );
    assert_eq!(unchanged["upToDate"], true);
    assert_eq!(
        serde_json::to_value(store.all_auto_tags().unwrap()).unwrap(),
        serde_json::to_value(rows).unwrap()
    );

    // Hold the existing persistence boundary so a real recalculation stays in flight.
    let (held_tx, held_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let blocker = std::thread::spawn(move || {
        let _persistence = TREE.persistence_lock.lock().unwrap();
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    held_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let worker = std::thread::spawn(|| tags.recalculate().unwrap());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !tags.recalculation_status().running {
        assert!(Instant::now() < deadline, "recalculation did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(status(client).await["running"], true);
    assert_eq!(status(client).await["upToDate"], false);
    assert_eq!(
        owner(client.put("/put/recalculate_auto_tags"), true)
            .dispatch()
            .await
            .status(),
        Status::Conflict
    );
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    assert!(worker.join().unwrap());

    let last_success = status(client).await["lastCompletedAt"].clone();
    let id = records[0].hash();
    store
        .put(
            id.as_str(),
            &model.model_id,
            0,
            &StoredEmbedding {
                thumbnail_version: 0,
                vector: vec![1.0],
            },
        )
        .unwrap();
    tags.photo_embedded(id.as_str());
    assert_eq!(status(client).await["upToDate"], false);
    assert_eq!(
        owner(client.put("/put/recalculate_auto_tags"), true)
            .dispatch()
            .await
            .status(),
        Status::InternalServerError
    );
    let failed = status(client).await;
    assert_eq!(failed["lastCompletedAt"], last_success);
    assert_eq!(failed["running"], false);
    assert_eq!(failed["upToDate"], false);
    assert!(failed["lastError"].is_string());
    store
        .put(
            id.as_str(),
            &model.model_id,
            0,
            &StoredEmbedding {
                thumbnail_version: 0,
                vector: vector.to_vec(),
            },
        )
        .unwrap();
    tags.photo_embedded(id.as_str());
    assert_eq!(
        owner(client.put("/put/recalculate_auto_tags"), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert!(status(client).await["lastError"].is_null());

    // Authored state and opted-out photos survive the explicit retry.
    TREE.store
        .set_auto_tags_enabled(&[id.as_str()], false)
        .unwrap();
    crate::operations::auto_tags::invalidate();
    assert_eq!(status(client).await["upToDate"], false);
    assert_eq!(
        owner(client.put("/put/recalculate_auto_tags"), true)
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert!(
        !TREE
            .store
            .auto_tag_settings()
            .unwrap()
            .enabled(id.as_str())
            .unwrap()
    );
    assert!(
        store
            .auto_tags(id.as_str())
            .unwrap()
            .unwrap()
            .tags
            .is_empty()
    );
    assert!(
        TREE.store
            .auto_tag_rejections_for(id.as_str())
            .unwrap()
            .contains(&"dog".to_owned())
    );
    assert!(
        TREE.store
            .reader()
            .unwrap()
            .get(id.as_str())
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains("cat")
    );
}
