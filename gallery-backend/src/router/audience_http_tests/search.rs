use super::*;
use crate::public::structure::expression::{AlbumFilterValue, FilterValue};
use serde_json::json;

async fn query(client: &Client, expression: Expression, sort: &str) -> PrefetchReturn {
    let expression = Expression::And(vec![Expression::Trashed(false), expression]);
    let response = owner(
        client
            .post(format!("/get/prefetch?sort={sort}"))
            .json(&expression),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

async fn renew(client: &Client, snapshot: &PrefetchReturn) -> PrefetchReturn {
    let response = owner(
        client
            .post("/post/renew-timestamp-token")
            .json(&json!({"token": snapshot.token})),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    let mut renewed = snapshot.clone();
    renewed.token = response.into_json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    renewed
}

async fn matching(client: &Client, expression: Expression, expected: &[&str]) {
    let expected = expected
        .iter()
        .map(|hash| (*hash).to_owned())
        .collect::<HashSet<_>>();
    for sort in ["descending", "ascending", "random"] {
        let snapshot = query(client, expression.clone(), sort).await;
        assert_eq!(
            hashes(client, &snapshot, true).await,
            expected,
            "{expression:?}, {sort}"
        );
    }
}

async fn compositions(client: &Client) {
    matching(
        client,
        Expression::Audience(Audience::Private),
        &[PRIVATE, MISSING, PRIVATE_VIDEO],
    )
    .await;
    matching(
        client,
        Expression::Audience(Audience::Public),
        &[PUBLIC, PUBLIC_VIDEO, ALBUM],
    )
    .await;
    matching(
        client,
        Expression::Not(Box::new(Expression::Audience(Audience::Public))),
        &[PRIVATE, MISSING, PRIVATE_VIDEO],
    )
    .await;
    matching(
        client,
        Expression::And(vec![
            Expression::Audience(Audience::Private),
            Expression::ExtType("image".into()),
            Expression::Tag(FilterValue::Value("common-tag".into())),
            Expression::Album(AlbumFilterValue::Value(id(ALBUM))),
        ]),
        &[PRIVATE],
    )
    .await;
    matching(
        client,
        Expression::Or(vec![
            Expression::Audience(Audience::Public),
            Expression::And(vec![
                Expression::Audience(Audience::Private),
                Expression::ExtType("video".into()),
            ]),
        ]),
        &[PUBLIC, PUBLIC_VIDEO, PRIVATE_VIDEO, ALBUM],
    )
    .await;

    // Album audience is independent of its members, including under a combined filter.
    TREE.store
        .set_album_audience(ALBUM, Audience::Private, false)
        .unwrap();
    matching(
        client,
        Expression::And(vec![
            Expression::Audience(Audience::Private),
            Expression::ExtType("album".into()),
        ]),
        &[ALBUM],
    )
    .await;
    matching(
        client,
        Expression::And(vec![
            Expression::Audience(Audience::Public),
            Expression::Album(AlbumFilterValue::Value(id(ALBUM))),
        ]),
        &[PUBLIC],
    )
    .await;
    TREE.store
        .set_album_audience(ALBUM, Audience::Public, false)
        .unwrap();
}

async fn reject_non_owner(client: &Client) {
    for expression in [
        json!({"Audience": "private"}),
        json!({"Audience": "public"}),
        json!({"Not": {"Audience": "private"}}),
        json!({"Or": [{"ExtType": "image"}, {"Not": {"And": [{"Audience": "private"}]}}]}),
    ] {
        let response = client
            .post("/get/prefetch")
            .json(&expression)
            .dispatch()
            .await;
        assert_eq!(
            response.status(),
            if crate::router::fairing::guard_gallery::public_gallery_enabled() {
                Status::BadRequest
            } else {
                Status::Unauthorized
            }
        );
        let response = client
            .post("/get/prefetch")
            .json(&expression)
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share"))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::BadRequest);
    }
}

async fn request_validation(client: &Client) {
    for body in [None, Some(""), Some(" \t\r\n "), Some("null")] {
        let mut request = client.post("/get/prefetch").header(ContentType::JSON);
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = owner(request, true).dispatch().await;
        assert_eq!(response.status(), Status::Ok, "empty filter {body:?}");
        let snapshot = response.into_json::<PrefetchReturn>().await.unwrap();
        assert_eq!(snapshot.prefetch.data_length, 7);
    }
    for body in [
        r#"{"Audience":"friends"}"#,
        r#"{"Audience":true}"#,
        r#"{"And":[{"Audience":"friends"}]}"#,
        "{",
        "null trailing garbage",
    ] {
        let response = owner(
            client
                .post("/get/prefetch")
                .header(ContentType::JSON)
                .body(body),
            true,
        )
        .dispatch()
        .await;
        assert_eq!(
            response.status(),
            Status::BadRequest,
            "invalid filter {body}"
        );
    }
}

async fn revision_checks(client: &Client) {
    let expression = Expression::Not(Box::new(Expression::Audience(Audience::Public)));
    let unfiltered = snapshot(client, true, "descending").await;
    let mut old_snapshots = Vec::new();
    for sort in ["descending", "ascending", "random"] {
        let original = query(client, expression.clone(), sort).await;
        let cached = query(client, expression.clone(), sort).await;
        if sort != "random" {
            assert_eq!(original.prefetch.timestamp, cached.prefetch.timestamp);
        }
        old_snapshots.extend([original, cached.clone(), renew(client, &cached).await]);
    }
    let response = owner(
        client
            .put("/put/edit_photo_audience")
            .json(&json!({"hash": PRIVATE, "audience": "public"})),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    for snapshot in &old_snapshots {
        for route in ["data", "rows", "scroll"] {
            assert_eq!(
                owner(client.get(snapshot_path(route, snapshot)), true)
                    .dispatch()
                    .await
                    .status(),
                Status::Unauthorized
            );
        }
        assert_eq!(
            owner(
                client
                    .post("/post/renew-timestamp-token")
                    .json(&json!({"token": snapshot.token})),
                true
            )
            .dispatch()
            .await
            .status(),
            Status::Unauthorized
        );
    }
    assert!(hashes(client, &unfiltered, true).await.contains(PRIVATE));
    matching(client, expression.clone(), &[MISSING, PRIVATE_VIDEO]).await;
    let fresh = query(client, expression.clone(), "descending").await;
    assert_ne!(
        fresh.prefetch.timestamp,
        old_snapshots[0].prefetch.timestamp
    );
    TREE.store
        .set_photo_audience(PRIVATE, Audience::Private)
        .unwrap();
    matching(client, expression, &[PRIVATE, MISSING, PRIVATE_VIDEO]).await;
}

pub(super) async fn checks(client: &Client) {
    request_validation(client).await;
    reject_non_owner(client).await;
    compositions(client).await;
    revision_checks(client).await;
}
