//! Real owner snapshot/listing/selection routes; no sidecar or new storage.
use super::*;
use crate::{
    operations::{autotag::BuiltinCatalog, embedding},
    storage::{
        auto_tags::{MaterializedTags, StoredAutoTag},
        embeddings::StoredEmbedding,
    },
};
use serde_json::json;

async fn review(client: &Client, concept: &str) -> (Value, Vec<String>) {
    let response = owner(
        client
            .post(format!("/get/prefetch?review_concept={concept}"))
            .json(&Value::Null),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_json::<Value>().await.unwrap();
    let snapshot: PrefetchReturn = serde_json::from_value(body.clone()).unwrap();
    let rows = owner(client.get(snapshot_path("data", &snapshot)), true)
        .dispatch()
        .await
        .into_json::<Vec<Value>>()
        .await
        .unwrap();
    let ids = rows
        .into_iter()
        .map(|row| row["abstractData"]["id"].as_str().unwrap().to_owned())
        .collect();
    (body, ids)
}

async fn decide(client: &Client, hash: &str, concept: &str, action: &str) {
    assert_eq!(
        owner(
            client
                .put("/put/auto_tag_decision")
                .json(&json!({"hash": hash, "concept": concept, "action": action})),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
}

#[allow(
    clippy::too_many_lines,
    reason = "One isolated owner/visitor and feedback acceptance sequence"
)]
pub(super) async fn checks(client: &Client) {
    let tags = embedding::service().unwrap().auto_tags.get().unwrap();
    let store = tags.test_store();
    let catalog = BuiltinCatalog::embedded().unwrap();
    // Stable current embeddings also establish the real classifier example counts.
    for hash in [PUBLIC, PRIVATE, TRASHED, MISSING] {
        store
            .put(
                hash,
                &catalog.model_id,
                0,
                &StoredEmbedding {
                    thumbnail_version: 7,
                    vector: vec![1.0, 0.0],
                },
            )
            .unwrap();
    }
    TREE.store
        .write(|writer| -> anyhow::Result<()> {
            let mut record = writer.get(MISSING)?.unwrap().into_value();
            record.tag_mut().extend(["Chat".into(), "Moka".into()]);
            writer.insert(&record)
        })
        .unwrap();
    let records = TREE
        .store
        .reader()
        .unwrap()
        .values()
        .unwrap()
        .map(|r| r.unwrap().into_value())
        .collect::<Vec<_>>();
    *TREE.state.write().unwrap() = TreeState::from_records(records);
    let row = |version, confidence| MaterializedTags {
        enabled: true,
        model_id: catalog.model_id.clone(),
        vocabulary_version: catalog.version.clone(),
        thumbnail_version: version,
        tags: vec![StoredAutoTag {
            concept: "cat".into(),
            label: "chat".into(),
            confidence,
            displayed: true,
        }],
    };
    store
        .put_auto_tags(&[
            (PUBLIC.into(), row(7, 0.95)),
            (PRIVATE.into(), row(7, 0.55)),
            (TRASHED.into(), row(7, 0.72)),
            (MISSING.into(), row(6, 0.1)),
            (PUBLIC_VIDEO.into(), row(7, 0.1)),
            ("deleted".into(), row(7, 0.1)),
        ])
        .unwrap();
    let (body, ids) = review(client, "cat").await;
    assert_eq!(ids, vec![PRIVATE, TRASHED, PUBLIC]);
    assert_eq!(
        body["conceptReview"],
        json!({"concept": "cat", "label": "chat", "positiveExamples": 1, "negativeExamples": 0, "learningMode": "pending"})
    );
    // Labels/aliases resolve to the same stable Concept; explicit sort/filter mixing is refused.
    assert_eq!(review(client, "Chat").await.1, ids);
    for path in [
        "/get/prefetch?review_concept=cat&sort=random",
        "/get/prefetch?review_concept=cat&semantic_selection=stale",
    ] {
        assert_eq!(
            owner(client.post(path).json(&Value::Null), true)
                .dispatch()
                .await
                .status(),
            Status::BadRequest
        );
    }
    assert_eq!(
        owner(
            client
                .post("/get/prefetch?review_concept=cat")
                .json(&json!({"Trashed": false})),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::BadRequest
    );
    assert_eq!(
        owner(
            client
                .post("/get/prefetch?review_concept=unknown")
                .json(&Value::Null),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::NotFound
    );
    for name in ["cat", "unknown"] {
        let path = format!("/get/prefetch?review_concept={name}");
        for request in [
            client.post(path.clone()).json(&Value::Null),
            client
                .post(path)
                .json(&Value::Null)
                .header(Header::new("x-album-id", ALBUM))
                .header(Header::new("x-share-id", "fixture-share")),
        ] {
            let response = request.dispatch().await;
            assert!([Status::Forbidden, Status::Unauthorized].contains(&response.status()));
            assert!(
                !response
                    .into_string()
                    .await
                    .unwrap()
                    .contains("positiveExamples")
            );
        }
    }
    // Disabled/stale/deleted/video predictions never enter the review, even if cached.
    TREE.store.set_auto_tags_enabled(&[PRIVATE], false).unwrap();
    assert_eq!(review(client, "cat").await.1, vec![TRASHED, PUBLIC]);
    TREE.store.set_auto_tags_enabled(&[PRIVATE], true).unwrap();
    let mut stale = row(6, 0.95);
    stale.vocabulary_version.push_str("-old");
    store.put_auto_tags(&[(PUBLIC.into(), stale)]).unwrap();
    assert_eq!(review(client, "cat").await.1, vec![PRIVATE, TRASHED]);
    store
        .put_auto_tags(&[(PUBLIC.into(), row(7, 0.95))])
        .unwrap();
    // Derived storage may still contain old rows while offline; authored decisions win.
    decide(client, PRIVATE, "cat", "reject").await;
    decide(client, PUBLIC, "cat", "confirm").await;
    let (body, ids) = review(client, "cat").await;
    assert_eq!(ids, vec![TRASHED]);
    assert_eq!(body["conceptReview"]["positiveExamples"], 2);
    assert_eq!(body["conceptReview"]["negativeExamples"], 1);
    let snapshot: PrefetchReturn = serde_json::from_value(body).unwrap();
    assert_eq!(owner(client.put("/put/auto_tag_decision").json(&json!({"timestamp": snapshot.prefetch.timestamp, "selection": {"mode":"allExcept","excludedIndices":[]}, "concept":"cat", "action":"reject"})), true).dispatch().await.status(), Status::Ok);
    assert!(review(client, "cat").await.1.is_empty());
    for hash in [PUBLIC, PRIVATE, TRASHED] {
        let body = owner(client.get(format!("/get/auto-tags/{hash}")), true)
            .dispatch()
            .await
            .into_json::<Value>()
            .await
            .unwrap();
        assert!(
            !body["autoTags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tag| tag["concept"] == "cat")
        );
    }
    let personal = crate::operations::autotag::learning::personal_id("Moka");
    let mut personal_row = row(7, 0.8);
    personal_row.tags = vec![StoredAutoTag {
        concept: personal.clone(),
        label: "Moka".into(),
        confidence: 0.8,
        displayed: true,
    }];
    store
        .put_auto_tags(&[(PUBLIC.into(), personal_row)])
        .unwrap();
    assert_eq!(review(client, "Moka").await.1, vec![PUBLIC]);
    assert_eq!(
        review(client, &personal).await.0["conceptReview"]["positiveExamples"],
        1
    );
    decide(client, PUBLIC, &personal, "reject").await;
    assert!(review(client, "Moka").await.1.is_empty());
}
