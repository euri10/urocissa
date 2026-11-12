use std::{collections::BTreeMap, sync::atomic::Ordering};

use super::*;
use crate::public::{
    db::tree::{VERSION_COUNT_TIMESTAMP, read_tags::SearchFacets},
    structure::{expression::AlbumFilterValue, response::row::ScrollBarData},
};

#[derive(Clone, Copy, Debug)]
enum Visitor {
    Owner,
    Anonymous,
    Share,
}

fn as_visitor(request: LocalRequest<'_>, visitor: Visitor) -> LocalRequest<'_> {
    match visitor {
        Visitor::Owner => owner(request, true),
        Visitor::Anonymous => request,
        Visitor::Share => request
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    }
}

async fn facets(client: &Client, visitor: Visitor, suffix: &str) -> SearchFacets {
    let response = as_visitor(
        client.get(format!("/get/get-search-facets{suffix}")),
        visitor,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok, "{visitor:?}");
    response.into_json().await.unwrap()
}

fn values(values: &[crate::public::db::tree::read_tags::FacetValueInfo]) -> BTreeMap<&str, usize> {
    values
        .iter()
        .map(|facet| (facet.value.as_str(), facet.count))
        .collect()
}

async fn facet_checks(client: &Client, revoked: bool) {
    // Repeated reads after owner warming must not retain private-only values or counts.
    for visitor in [Visitor::Owner, Visitor::Anonymous, Visitor::Share] {
        let expected = match visitor {
            Visitor::Owner => (
                vec![
                    ("common-tag", 5),
                    ("outside-share-tag", 1),
                    ("private-search-tag", 1),
                    ("private-video-tag", 1),
                    ("public-search-tag", 1),
                    ("unshared-private-tag", 1),
                ],
                vec![("CANON", 4), ("Private make", 1)],
                vec![
                    ("Private model", 2),
                    ("Unshared model", 1),
                    ("Visible model", 2),
                ],
            ),
            Visitor::Anonymous => {
                let count = if revoked { 1 } else { 2 };
                let mut tags = vec![("common-tag", count), ("outside-share-tag", 1)];
                if !revoked {
                    tags.push(("public-search-tag", 1));
                }
                (tags, vec![("Canon", count)], vec![("Visible model", count)])
            }
            Visitor::Share => (
                vec![
                    ("common-tag", 3),
                    ("private-search-tag", 1),
                    ("private-video-tag", 1),
                    ("public-search-tag", 1),
                ],
                vec![("CANON", 2), ("Private make", 1)],
                vec![("Private model", 2), ("Visible model", 1)],
            ),
        };
        for _ in 0..2 {
            let actual = facets(client, visitor, "?trashed=false").await;
            assert_eq!(
                values(&actual.tags),
                expected.0.iter().copied().collect(),
                "{visitor:?} tags"
            );
            assert_eq!(
                values(&actual.makes),
                expected.1.iter().copied().collect(),
                "{visitor:?} makes"
            );
            assert_eq!(
                values(&actual.models),
                expected.2.iter().copied().collect(),
                "{visitor:?} models"
            );
            if !matches!(visitor, Visitor::Owner) {
                assert_eq!(facets(client, visitor, "").await, actual);
                assert_eq!(facets(client, visitor, "?trashed=true").await, actual);
            }
        }
    }
    let all = facets(client, Visitor::Owner, "").await;
    assert_eq!(values(&all.tags)["common-tag"], 6);
    assert_eq!(values(&all.tags)["trash-tag"], 1);
    let trash = facets(client, Visitor::Owner, "?trashed=true").await;
    assert_eq!(
        values(&trash.tags),
        BTreeMap::from([("common-tag", 1), ("trash-tag", 1)])
    );
    assert_eq!(values(&trash.makes), BTreeMap::from([("Trash make", 1)]));
    assert_eq!(values(&trash.models), BTreeMap::from([("Trash model", 1)]));
}

async fn media_snapshot(client: &Client, visitor: Visitor, sort: &str) -> PrefetchReturn {
    let query = Expression::And(vec![
        Expression::Trashed(false),
        Expression::Not(Box::new(Expression::ExtType("album".into()))),
    ]);
    let response = as_visitor(
        client
            .post(format!("/get/prefetch?sort={sort}"))
            .header(ContentType::JSON)
            .body(serde_json::to_string(&query).unwrap()),
        visitor,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

async fn assert_months(
    client: &Client,
    visitor: Visitor,
    snapshot: &PrefetchReturn,
    months: &[usize],
) {
    assert_eq!(
        snapshot.prefetch.data_length,
        months.len(),
        "{visitor:?} count"
    );
    let response = as_visitor(client.get(snapshot_path("scroll", snapshot)), visitor)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let expected: Vec<_> = months
        .iter()
        .enumerate()
        .map(|(index, month)| ScrollBarData {
            year: 2024,
            month: *month,
            index,
        })
        .collect();
    assert_eq!(
        response.into_json::<Vec<ScrollBarData>>().await.unwrap(),
        expected,
        "{visitor:?} months"
    );
}

async fn album_counts(client: &Client, visitor: Visitor, count: usize) {
    let response = as_visitor(client.get(format!("/get/resource/{ALBUM}")), visitor)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok, "{visitor:?} album");
    let resource: Value = response.into_json().await.unwrap();
    assert_eq!(resource["data"]["abstractData"]["itemCount"], count);
    assert_eq!(resource["data"]["abstractData"]["itemSize"], count * 13);
    let snapshot: PrefetchReturn = serde_json::from_value(resource).unwrap();
    let response = as_visitor(client.get(snapshot_path("data", &snapshot)), visitor)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok, "{visitor:?} resource token");
    let data: Vec<Value> = response.into_json().await.unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["abstractData"]["itemCount"], count);
    assert_eq!(data[0]["abstractData"]["itemSize"], count * 13);

    // The collection count must agree with the album's projected content count.
    let query = Expression::And(vec![
        Expression::Trashed(false),
        Expression::Album(AlbumFilterValue::Value(id(ALBUM))),
    ]);
    let response = as_visitor(
        client
            .post("/get/prefetch")
            .header(ContentType::JSON)
            .body(serde_json::to_string(&query).unwrap()),
        visitor,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response
            .into_json::<PrefetchReturn>()
            .await
            .unwrap()
            .prefetch
            .data_length,
        count
    );
}

pub(super) async fn checks(client: &Client, revoked: bool) {
    facet_checks(client, revoked).await;
    for visitor in [Visitor::Owner, Visitor::Anonymous, Visitor::Share] {
        let months = match visitor {
            Visitor::Owner => vec![5, 4, 3, 2, 1],
            Visitor::Anonymous if revoked => vec![2],
            Visitor::Anonymous => vec![2, 1],
            Visitor::Share => vec![5, 3, 1],
        };
        for sort in ["descending", "ascending"] {
            let expected = if sort == "ascending" {
                months.iter().copied().rev().collect::<Vec<_>>()
            } else {
                months.clone()
            };
            let first = media_snapshot(client, visitor, sort).await;
            assert_months(client, visitor, &first, &expected).await;
            let warmed = media_snapshot(client, visitor, sort).await;
            assert_eq!(warmed.prefetch.timestamp, first.prefetch.timestamp);
            assert_months(client, visitor, &warmed, &expected).await;
        }
        assert_eq!(
            media_snapshot(client, visitor, "random")
                .await
                .prefetch
                .data_length,
            months.len()
        );
        album_counts(
            client,
            visitor,
            match visitor {
                Visitor::Anonymous => usize::from(!revoked),
                _ => 3,
            },
        )
        .await;
    }
}

pub(super) async fn private_gallery_checks(client: &Client) {
    TREE.store
        .set_album_audience(ALBUM, Audience::Private, false)
        .unwrap();
    assert_eq!(
        client
            .get("/get/get-search-facets")
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client.get("/get/get-albums").dispatch().await.status(),
        Status::Unauthorized
    );
    assert_album_list(client, Visitor::Owner, &[ALBUM]).await;
    assert_album_list(client, Visitor::Share, &[ALBUM]).await;
    assert_eq!(
        values(&facets(client, Visitor::Owner, "").await.tags)["common-tag"],
        6
    );
    assert_eq!(
        values(&facets(client, Visitor::Share, "").await.tags)["common-tag"],
        3
    );
    let shared = media_snapshot(client, Visitor::Share, "descending").await;
    assert_months(client, Visitor::Share, &shared, &[5, 3, 1]).await;
    album_counts(client, Visitor::Share, 3).await;
}

async fn assert_album_list(client: &Client, visitor: Visitor, expected: &[&str]) {
    let response = as_visitor(client.get("/get/get-albums"), visitor)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let albums: Vec<crate::router::get::get_list::AlbumInfo> = response.into_json().await.unwrap();
    assert_eq!(
        albums
            .iter()
            .map(|album| album.album_id.as_str())
            .collect::<HashSet<_>>(),
        expected.iter().copied().collect(),
        "{visitor:?} album list"
    );
    for album in &albums {
        if matches!(visitor, Visitor::Owner) {
            if album.album_id == ALBUM {
                assert!(album.share_list.contains_key("fixture-share"));
            }
        } else {
            assert!(album.share_list.is_empty());
        }
    }
}

async fn album_list_checks(client: &Client) {
    const OTHER_ALBUM: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const TRASHED_ALBUM: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    let mut other = Album::new(id(OTHER_ALBUM), Some("Other fixture album".into()));
    other.share_list.insert(id("other-share"), Share::default());
    let mut trash =
        Album::new(id(TRASHED_ALBUM), Some("Trashed fixture album".into())).into_abstract_data();
    trash.set_trashed(true);
    update_records(&[other.into_abstract_data(), trash]);
    TREE.store
        .set_album_audience(OTHER_ALBUM, Audience::Public, false)
        .unwrap();
    assert_album_list(client, Visitor::Owner, &[ALBUM, OTHER_ALBUM]).await;
    assert_album_list(client, Visitor::Anonymous, &[ALBUM, OTHER_ALBUM]).await;
    assert_album_list(client, Visitor::Share, &[ALBUM]).await;

    // Trashing removes destinations, while Trash and Restore retain album contents.
    for is_trashed in [true, false] {
        let response = owner(client.get(format!("/get/resource/{ALBUM}")), true)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let resource: Value = response.into_json().await.unwrap();
        assert_eq!(resource["data"]["abstractData"]["isTrashed"], !is_trashed);
        let response = owner(
            client.put("/put/edit_flags").json(&serde_json::json!({
                "indexArray": [0],
                "timestamp": resource["prefetch"]["timestamp"],
                "isTrashed": is_trashed,
            })),
            true,
        )
        .dispatch()
        .await;
        assert_eq!(response.status(), Status::Ok);
        let expected = if is_trashed {
            vec![OTHER_ALBUM]
        } else {
            vec![ALBUM, OTHER_ALBUM]
        };
        assert_album_list(client, Visitor::Owner, &expected).await;
        assert_album_list(client, Visitor::Anonymous, &expected).await;
        let query = Expression::And(vec![
            Expression::ExtType("album".into()),
            Expression::Trashed(is_trashed),
        ]);
        let response = owner(client.post("/get/prefetch").json(&query), true)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let snapshot: PrefetchReturn = response.into_json().await.unwrap();
        assert!(hashes(client, &snapshot, true).await.contains(ALBUM));
        album_counts(client, Visitor::Owner, 3).await;
        assert_eq!(
            TREE.store.photo_audiences().unwrap().get(PRIVATE).unwrap(),
            Audience::Private
        );
    }
    assert_album_list(client, Visitor::Share, &[ALBUM]).await;

    // A share grants its private album without granting unrelated public albums.
    TREE.store
        .set_album_audience(ALBUM, Audience::Private, false)
        .unwrap();
    assert_album_list(client, Visitor::Anonymous, &[OTHER_ALBUM]).await;
    assert_album_list(client, Visitor::Share, &[ALBUM]).await;
    assert_eq!(
        client
            .get(format!("/get/resource/{ALBUM}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    album_counts(client, Visitor::Share, 3).await;
    TREE.store
        .set_album_audience(ALBUM, Audience::Public, false)
        .unwrap();
}

async fn hidden_resource_checks(client: &Client) {
    let response = as_visitor(
        client.get(format!("/get/resource/{PRIVATE}")),
        Visitor::Share,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    let resource: Value = response.into_json().await.unwrap();
    assert_eq!(
        resource["data"]["abstractData"]["tags"],
        serde_json::json!([])
    );
    assert_eq!(
        resource["data"]["abstractData"]["exifVec"],
        serde_json::json!({})
    );
    let snapshot: PrefetchReturn = serde_json::from_value(resource).unwrap();
    let claims = jsonwebtoken::decode::<crate::router::claims::claims_timestamp::ClaimsTimestamp>(
        &snapshot.token,
        &jsonwebtoken::DecodingKey::from_secret(KEY),
        &jsonwebtoken::Validation::default(),
    )
    .unwrap()
    .claims;
    assert_eq!(
        claims.viewer,
        crate::public::structure::audience::Viewer::Share(id(ALBUM))
    );
    let share = claims.resolved_share_opt.unwrap();
    assert_eq!(share.album_id, id(ALBUM));
    assert!(!share.share.show_metadata);
    let response = as_visitor(client.get(snapshot_path("data", &snapshot)), Visitor::Share)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let data: Vec<Value> = response.into_json().await.unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["abstractData"]["id"], PRIVATE);
    assert_eq!(data[0]["abstractData"]["tags"], serde_json::json!([]));
    assert_eq!(data[0]["abstractData"]["exifVec"], serde_json::json!({}));
}

fn update_records(records: &[AbstractData]) {
    TREE.store
        .write(|writer| -> anyhow::Result<()> {
            for record in records {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    TREE.state
        .write()
        .unwrap()
        .apply_batch(records, &HashSet::new());
    VERSION_COUNT_TIMESTAMP.fetch_add(1, Ordering::Relaxed);
}

#[allow(clippy::too_many_lines)]
pub(super) async fn share_revocation_checks(client: &Client, records: &[AbstractData]) {
    album_list_checks(client).await;
    let mut album = records
        .iter()
        .find(|data| data.hash().as_str() == ALBUM)
        .unwrap()
        .clone();
    let AbstractData::Album(ref mut record) = album else {
        unreachable!()
    };
    record
        .metadata
        .share_list
        .get_mut("fixture-share")
        .unwrap()
        .show_metadata = false;
    update_records(&[album.clone()]);
    let hidden = facets(client, Visitor::Share, "").await;
    assert!(hidden.tags.is_empty() && hidden.makes.is_empty() && hidden.models.is_empty());
    hidden_resource_checks(client).await;
    let AbstractData::Album(ref mut record) = album else {
        unreachable!()
    };
    record
        .metadata
        .share_list
        .get_mut("fixture-share")
        .unwrap()
        .show_metadata = true;
    update_records(&[album.clone()]);

    let previous = media_snapshot(client, Visitor::Share, "descending").await;
    assert_months(client, Visitor::Share, &previous, &[5, 3, 1]).await;
    let mut removed = records
        .iter()
        .find(|data| data.hash().as_str() == PRIVATE)
        .unwrap()
        .clone();
    removed.albums_mut().unwrap().remove(&id(ALBUM));
    let AbstractData::Album(ref mut record) = album else {
        unreachable!()
    };
    record.metadata.item_count = 2;
    record.metadata.item_size = 26;
    // A membership mutation changes no audience revision; cached month data still must close.
    update_records(&[removed, album.clone()]);
    for route in ["data", "rows", "scroll"] {
        let response = as_visitor(client.get(snapshot_path(route, &previous)), Visitor::Share)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::NotFound, "stale shared {route}");
    }
    let current = media_snapshot(client, Visitor::Share, "descending").await;
    assert_months(client, Visitor::Share, &current, &[5, 1]).await;
    let current_facets = facets(client, Visitor::Share, "").await;
    assert_eq!(
        values(&current_facets.tags),
        BTreeMap::from([
            ("common-tag", 2),
            ("private-video-tag", 1),
            ("public-search-tag", 1)
        ])
    );
    album_counts(client, Visitor::Share, 2).await;
    let complete = media_snapshot(client, Visitor::Owner, "descending").await;
    assert_months(client, Visitor::Owner, &complete, &[5, 4, 3, 2, 1]).await;

    let AbstractData::Album(ref mut record) = album else {
        unreachable!()
    };
    record.metadata.share_list.clear();
    update_records(&[album]);
    for route in [
        "/get/get-search-facets".to_owned(),
        "/get/get-albums".to_owned(),
        format!("/get/resource/{ALBUM}"),
        snapshot_path("data", &current),
        snapshot_path("rows", &current),
        snapshot_path("scroll", &current),
    ] {
        assert_eq!(
            as_visitor(client.get(route), Visitor::Share)
                .dispatch()
                .await
                .status(),
            Status::NotFound
        );
    }
    assert_eq!(
        as_visitor(
            client.post("/get/prefetch").header(ContentType::JSON),
            Visitor::Share
        )
        .dispatch()
        .await
        .status(),
        Status::NotFound
    );
    assert_eq!(
        as_visitor(
            client
                .post("/post/renew-timestamp-token")
                .header(ContentType::JSON)
                .body(serde_json::json!({"token": current.token}).to_string()),
            Visitor::Share
        )
        .dispatch()
        .await
        .status(),
        Status::NotFound
    );
}
