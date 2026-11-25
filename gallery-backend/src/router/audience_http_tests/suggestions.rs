use super::*;
use crate::{
    operations::suggestions::{self, VERSION},
    public::structure::expression::{FilterValue, TagSource},
    storage::suggestions::{SuggestedTags, SuggestionStore},
};
use serde_json::json;

async fn labels(client: &Client) -> Vec<String> {
    let response = owner(client.get(format!("/get/suggested-tags/{PUBLIC}")), true)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json::<Value>().await.unwrap()["suggestedTags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

async fn source_count(client: &Client, name: &str, source: TagSource) -> usize {
    let response = owner(
        client.post("/get/prefetch").json(&Expression::And(vec![
            Expression::Tag(FilterValue::Value(name.into())),
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

pub(super) async fn checks(client: &Client) {
    let store = SuggestionStore::open(
        &crate::public::constant::storage::get_data_path().join("db/suggestions.redb"),
    )
    .unwrap();
    suggestions::install_test_service(store.clone());
    let auto_store = crate::operations::embedding::service()
        .unwrap()
        .auto_tags
        .get()
        .unwrap()
        .test_store();
    let catalog = crate::operations::autotag::BuiltinCatalog::embedded().unwrap();
    auto_store
        .put_auto_tags(&[(
            PUBLIC.into(),
            crate::storage::auto_tags::MaterializedTags {
                enabled: true,
                model_id: catalog.model_id,
                vocabulary_version: catalog.version,
                thumbnail_version: 7,
                tags: vec![crate::storage::auto_tags::StoredAutoTag {
                    concept: "cat".into(),
                    label: "cat".into(),
                    confidence: 0.9,
                    displayed: true,
                }],
            },
        )])
        .unwrap();
    let row = SuggestedTags {
        version: VERSION.into(),
        thumbnail_version: 7,
        labels: vec!["umbrella".into(), "backpack".into(), "cat".into()],
    };
    store.put(PUBLIC, &row).unwrap();
    store
        .put(
            PRIVATE,
            &SuggestedTags {
                thumbnail_version: 6,
                ..row.clone()
            },
        )
        .unwrap();

    for request in [
        client.get(format!("/get/suggested-tags/{PUBLIC}")),
        client
            .get(format!("/get/suggested-tags/{PUBLIC}"))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    ] {
        assert_eq!(request.dispatch().await.status(), Status::Unauthorized);
    }
    assert_eq!(labels(client).await, vec!["umbrella", "backpack", "cat"]);
    assert_eq!(
        source_count(client, "umbrella", TagSource::Suggested).await,
        1
    );
    assert_eq!(source_count(client, "umbrella", TagSource::Auto).await, 0);
    assert_eq!(source_count(client, "cat", TagSource::Auto).await, 1);
    assert_eq!(source_count(client, "cat", TagSource::Suggested).await, 1);
    let facets = owner(client.get("/get/get-search-facets"), true)
        .dispatch()
        .await
        .into_json::<Value>()
        .await
        .unwrap();
    assert!(
        facets["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|facet| facet["value"] == "umbrella" && facet["suggestedCount"] == 1)
    );
    if crate::router::fairing::guard_gallery::public_gallery_enabled() {
        let public = client
            .get("/get/get-search-facets")
            .dispatch()
            .await
            .into_string()
            .await
            .unwrap();
        assert!(!public.contains("umbrella") && !public.contains("suggestedCount"));
        let response = client
            .post("/get/prefetch")
            .json(&Expression::Source(TagSource::Suggested))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Forbidden);
    }

    for (label, action) in [("umbrella", "reject"), ("backpack", "confirm")] {
        let response = owner(
            client.put("/put/concept_candidate_decision").json(&json!({
                "hash": PUBLIC, "label": label, "action": action
            })),
            true,
        )
        .dispatch()
        .await;
        assert_eq!(response.status(), Status::Ok);
    }
    // Reprocessing the same photo cannot restore an owner decision.
    store.put(PUBLIC, &row).unwrap();
    assert_eq!(labels(client).await, vec!["cat"]);
    assert_eq!(
        source_count(client, "umbrella", TagSource::Suggested).await,
        0
    );
    assert_eq!(
        source_count(client, "backpack", TagSource::Suggested).await,
        0
    );
    assert_eq!(source_count(client, "backpack", TagSource::User).await, 1);
    assert!(
        TREE.store
            .auto_tag_rejections_for(PUBLIC)
            .unwrap()
            .contains(&crate::operations::tag_feedback::concept_id("umbrella"))
    );
}
