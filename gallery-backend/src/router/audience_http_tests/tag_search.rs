//! The same authored fixture before/after derived predictions: visitors keep exact bytes.
use super::*;
use crate::{
    operations::autotag::BuiltinCatalog,
    public::structure::expression::{FilterValue, TagSource},
    storage::{
        auto_tags::{MaterializedTags, StoredAutoTag},
        embeddings::EmbeddingStore,
    },
};

fn visitor(request: LocalRequest<'_>, shared: bool) -> LocalRequest<'_> {
    if shared {
        request
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share"))
    } else {
        request
    }
}

async fn query(client: &Client, expression: Expression, sort: &str) -> HashSet<String> {
    let response = owner(
        client
            .post(format!("/get/prefetch?sort={sort}"))
            .json(&expression),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok, "{expression:?}");
    hashes(
        client,
        &response.into_json::<PrefetchReturn>().await.unwrap(),
        true,
    )
    .await
}

fn tag(name: &str, source: Option<TagSource>, trash: Option<bool>) -> Expression {
    let mut items = vec![Expression::Tag(FilterValue::Value(name.into()))];
    if let Some(source) = source {
        items.push(Expression::Source(source));
    }
    if let Some(trash) = trash {
        items.push(Expression::Trashed(trash));
    }
    Expression::And(items)
}

async fn public_bytes(client: &Client) -> Vec<String> {
    let mut bytes = Vec::new();
    for shared in [false, true] {
        if !shared && !crate::router::fairing::guard_gallery::public_gallery_enabled() {
            continue;
        }
        let response = visitor(client.get("/get/get-search-facets"), shared)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        bytes.push(response.into_string().await.unwrap());
        for name in ["cigogne", "stork", "oiseau", "bird", "common-tag"] {
            let response = visitor(
                client
                    .post("/get/prefetch")
                    .json(&tag(name, None, Some(false))),
                shared,
            )
            .dispatch()
            .await;
            assert_eq!(response.status(), Status::Ok);
            let snapshot = response.into_json::<PrefetchReturn>().await.unwrap();
            let response = visitor(client.get(snapshot_path("data", &snapshot)), shared)
                .dispatch()
                .await;
            assert_eq!(response.status(), Status::Ok);
            let body = response.into_string().await.unwrap();
            assert!(!body.contains("autoTags"));
            let mut data: Vec<Value> = serde_json::from_str(&body).unwrap();
            for row in &mut data {
                // Capability JWTs contain the request time; authored tags are a HashSet.
                // Compare metadata bytes after normalizing only these existing variations.
                row.as_object_mut().unwrap().remove("token");
                for key in ["tags", "albums"] {
                    if let Some(values) = row["abstractData"][key].as_array_mut() {
                        values.sort_unstable_by_key(Value::to_string);
                    }
                }
            }
            bytes.push(serde_json::to_string(&data).unwrap());
            for route in ["scroll", "rows"] {
                if route == "rows" && data.is_empty() {
                    continue;
                }
                let response = visitor(client.get(snapshot_path(route, &snapshot)), shared)
                    .dispatch()
                    .await;
                assert_eq!(response.status(), Status::Ok);
                bytes.push(response.into_string().await.unwrap());
            }
        }
    }
    bytes
}

#[expect(
    clippy::too_many_lines,
    reason = "One isolated fixture sequence compares visitor bytes before and after derived changes"
)]
pub(super) async fn checks(client: &Client, store: &EmbeddingStore) {
    let originals = [PUBLIC, PRIVATE, MISSING, TRASHED].map(|id| {
        TREE.store
            .reader()
            .unwrap()
            .get(id)
            .unwrap()
            .unwrap()
            .into_value()
    });
    let mut records = originals.clone();
    records[0].tag_mut().insert("cigogne".into());
    records[0].tag_mut().insert("antifa".into());
    records[1].tag_mut().insert("stork".into());
    // Persist only authored fixtures; derived changes below must not alter visitor bytes.
    TREE.store
        .write(|writer| -> anyhow::Result<()> {
            for record in &records {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    // Replace through one state rebuild so tag indexes and query epochs agree.
    let all = TREE
        .store
        .reader()
        .unwrap()
        .values()
        .unwrap()
        .map(|r| r.unwrap().into_value())
        .collect::<Vec<_>>();
    *TREE.state.write().unwrap() = TreeState::from_records(all);
    let before = public_bytes(client).await;
    let catalog = BuiltinCatalog::embedded().unwrap();
    let row = |version, vocabulary: &str, tags: &[(&str, bool)]| MaterializedTags {
        enabled: true,
        model_id: catalog.model_id.clone(),
        vocabulary_version: vocabulary.into(),
        thumbnail_version: version,
        tags: tags
            .iter()
            .map(|(concept, displayed)| StoredAutoTag {
                concept: if *concept == "antifa" {
                    crate::operations::autotag::learning::personal_id(concept)
                } else {
                    (*concept).into()
                },
                label: (*concept).into(),
                confidence: 0.9,
                displayed: *displayed,
            })
            .collect(),
    };
    let rows = vec![
        (
            PUBLIC.into(),
            row(
                7,
                &catalog.version,
                &[("stork", false), ("bird", false), ("animal", false)],
            ),
        ),
        (PRIVATE.into(), row(7, &catalog.version, &[("bird", false)])),
        (
            MISSING.into(),
            row(
                7,
                &catalog.version,
                &[
                    ("stork", false),
                    ("bird", false),
                    ("animal", false),
                    ("antifa", false),
                ],
            ),
        ),
        (
            TRASHED.into(),
            row(
                7,
                &catalog.version,
                &[("stork", true), ("bird", false), ("animal", false)],
            ),
        ),
        (
            PUBLIC_VIDEO.into(),
            row(7, &catalog.version, &[("stork", true)]),
        ),
        (
            PRIVATE_VIDEO.into(),
            row(7, &catalog.version, &[("stork", true)]),
        ),
        (
            "deleted-photo".into(),
            row(7, &catalog.version, &[("stork", true)]),
        ),
    ];
    store.put_auto_tags(&rows).unwrap();
    let model = store.model().unwrap().unwrap();
    let mut changed_model = model.clone();
    changed_model.model_id.push_str("-changed");
    store.set_model(&changed_model).unwrap();
    assert!(
        query(
            client,
            tag("stork", Some(TagSource::Auto), Some(false)),
            "descending"
        )
        .await
        .is_empty()
    );
    store.set_model(&model).unwrap();
    for (source, expected) in [
        (None, vec![PUBLIC, MISSING]),
        (Some(TagSource::User), vec![PUBLIC]),
        (Some(TagSource::Auto), vec![MISSING]),
    ] {
        assert_eq!(
            query(client, tag("antifa", source, Some(false)), "descending").await,
            expected.into_iter().map(str::to_owned).collect()
        );
    }
    assert_eq!(
        query(
            client,
            Expression::And(vec![
                Expression::Tag(FilterValue::Exists(false)),
                Expression::Source(TagSource::Auto),
                Expression::Trashed(false)
            ]),
            "descending"
        )
        .await,
        HashSet::from([
            PUBLIC_VIDEO.to_owned(),
            PRIVATE_VIDEO.to_owned(),
            ALBUM.to_owned()
        ])
    );
    // Source qualifiers propagate through Boolean operands, but not across OR branches.
    let without_auto_birds = Expression::And(vec![
        Expression::Source(TagSource::Auto),
        Expression::Not(Box::new(Expression::Tag(FilterValue::Value("bird".into())))),
        Expression::Trashed(false),
    ]);
    assert_eq!(
        query(client, without_auto_birds, "descending").await,
        HashSet::from([
            PUBLIC_VIDEO.to_owned(),
            PRIVATE_VIDEO.to_owned(),
            ALBUM.to_owned()
        ])
    );
    let either = Expression::Or(vec![
        tag("stork", Some(TagSource::User), Some(false)),
        tag("stork", Some(TagSource::Auto), Some(false)),
    ]);
    assert_eq!(
        query(client, either, "descending").await,
        HashSet::from([PUBLIC.to_owned(), PRIVATE.to_owned(), MISSING.to_owned()])
    );
    assert_eq!(
        query(client, Expression::Source(TagSource::Auto), "descending").await,
        HashSet::from([
            PUBLIC.to_owned(),
            PRIVATE.to_owned(),
            MISSING.to_owned(),
            TRASHED.to_owned()
        ])
    );
    assert!(
        query(
            client,
            Expression::And(vec![
                Expression::Source(TagSource::User),
                Expression::Source(TagSource::Auto)
            ]),
            "descending"
        )
        .await
        .is_empty()
    );
    for name in ["cigogne", "stork", "oiseau", "bird", "animal", " CIGOGNE "] {
        for source in [None, Some(TagSource::User), Some(TagSource::Auto)] {
            let expected: HashSet<_> = match source {
                None => vec![PUBLIC, PRIVATE, MISSING],
                Some(TagSource::User) => vec![PUBLIC, PRIVATE],
                Some(TagSource::Auto) if ["cigogne", "stork", " CIGOGNE "].contains(&name) => {
                    vec![MISSING]
                }
                Some(TagSource::Auto) if ["oiseau", "bird"].contains(&name) => {
                    vec![PUBLIC, PRIVATE, MISSING]
                }
                Some(TagSource::Auto) => vec![PUBLIC, MISSING],
            }
            .into_iter()
            .map(str::to_owned)
            .collect();
            for sort in ["descending", "ascending", "random"] {
                assert_eq!(
                    query(client, tag(name, source, Some(false)), sort).await,
                    expected,
                    "{name} {source:?}"
                );
            }
        }
    }
    // Every advertised count is exactly reproducible with tag + source + trash filters.
    for trash in [None, Some(false), Some(true)] {
        let url = trash.map_or_else(
            || "/get/get-search-facets".into(),
            |v| format!("/get/get-search-facets?trashed={v}"),
        );
        let facets = owner(client.get(url), true)
            .dispatch()
            .await
            .into_json::<crate::public::db::tree::read_tags::SearchFacets>()
            .await
            .unwrap();
        if trash.is_none() {
            let snapshot = snapshot(client, true, "descending").await;
            let response = owner(
                client.put("/put/edit_tag").json(&serde_json::json!({
                    "timestamp": snapshot.prefetch.timestamp, "index_array": [],
                    "addTagsArray": [], "removeTagsArray": []
                })),
                true,
            )
            .dispatch()
            .await;
            assert_eq!(response.status(), Status::Ok);
            assert_eq!(
                response
                    .into_json::<Vec<crate::public::db::tree::read_tags::FacetValueInfo>>()
                    .await
                    .unwrap(),
                facets.tags
            );
        }
        for facet in facets.tags {
            for (source, count) in [
                (None, facet.count),
                (Some(TagSource::User), facet.user_count.unwrap()),
                (Some(TagSource::Auto), facet.auto_count.unwrap()),
            ] {
                assert_eq!(
                    query(client, tag(&facet.value, source, trash), "descending")
                        .await
                        .len(),
                    count,
                    "{facet:?} {source:?}"
                );
            }
        }
    }
    assert_eq!(public_bytes(client).await, before);
    for shared in [false, true] {
        if !shared && !crate::router::fairing::guard_gallery::public_gallery_enabled() {
            continue;
        }
        for expression in [
            Expression::Source(TagSource::Auto),
            Expression::Not(Box::new(Expression::Source(TagSource::User))),
        ] {
            assert_eq!(
                visitor(client.post("/get/prefetch").json(&expression), shared)
                    .dispatch()
                    .await
                    .status(),
                Status::Forbidden
            );
        }
    }
    // New searches see off flags, stale rows and materialization changes, without an authored edit.
    TREE.store.set_auto_tags_enabled(&[MISSING], false).unwrap();
    assert_eq!(
        query(
            client,
            tag("stork", Some(TagSource::Auto), Some(false)),
            "descending"
        )
        .await,
        // PUBLIC's User tag already aliases stork; its old Auto row is suppressed.
        HashSet::new()
    );
    TREE.store.set_auto_tags_enabled(&[MISSING], true).unwrap();
    store
        .put_auto_tags(&[
            (PUBLIC.into(), row(8, &catalog.version, &[("stork", true)])),
            (
                MISSING.into(),
                row(7, "stale-vocabulary", &[("stork", true)]),
            ),
        ])
        .unwrap();
    assert!(
        query(
            client,
            tag("stork", Some(TagSource::Auto), Some(false)),
            "descending"
        )
        .await
        .is_empty()
    );
    // Clean fixtures for the existing audience/aggregate tests that follow.
    store
        .put_auto_tags(
            &rows
                .into_iter()
                .map(|(id, mut row)| {
                    row.tags.clear();
                    (id, row)
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    TREE.store
        .write(|writer| -> anyhow::Result<()> {
            for record in &originals {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    let all = TREE
        .store
        .reader()
        .unwrap()
        .values()
        .unwrap()
        .map(|r| r.unwrap().into_value())
        .collect::<Vec<_>>();
    *TREE.state.write().unwrap() = TreeState::from_records(all);
}
