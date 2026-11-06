use super::*;
use serde_json::json;

async fn change(client: &Client, body: Value, is_owner: bool) -> Status {
    owner(client.put("/put/edit_photo_audience").json(&body), is_owner)
        .dispatch()
        .await
        .status()
}

async fn resource(client: &Client, hash: &str, is_owner: bool) -> Value {
    let response = owner(client.get(format!("/get/resource/{hash}")), is_owner)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

async fn check_mutation_guards(client: &Client) {
    let body = json!({"hash": PRIVATE, "audience": "public"});
    let revision = TREE.store.photo_audiences().unwrap().revision;
    assert_eq!(
        change(client, body.clone(), false).await,
        Status::Unauthorized
    );
    assert_eq!(
        client
            .put("/put/edit_photo_audience")
            .json(&body)
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share"))
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
        change(client, body.clone(), true).await,
        Status::MethodNotAllowed
    );
    APP_CONFIG
        .get()
        .unwrap()
        .write()
        .unwrap()
        .public
        .read_only_mode = false;
    assert_eq!(TREE.store.photo_audiences().unwrap().revision, revision);
}

async fn check_owner_snapshots(
    client: &Client,
    before: &PrefetchReturn,
    direct_before: &PrefetchReturn,
    audience: &str,
) {
    for route in ["data", "rows", "scroll"] {
        assert_eq!(
            owner(client.get(snapshot_path(route, before)), true)
                .dispatch()
                .await
                .status(),
            Status::Ok
        );
    }
    for snapshot in [before, direct_before] {
        let response = owner(client.get(snapshot_path("data", snapshot)), true)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let data = response.into_json::<Vec<Value>>().await.unwrap();
        assert_eq!(
            data.iter()
                .find(|item| item["abstractData"]["id"] == PRIVATE)
                .unwrap()["audience"],
            audience
        );
    }
    let response = owner(
        client
            .post("/post/renew-timestamp-token")
            .json(&json!({"token": before.token})),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    let mut renewed = before.clone();
    renewed.token = response.into_json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        hashes(client, &renewed, true).await,
        hashes(client, before, true).await
    );
    // Owner capabilities still require an authenticated owner after renewal.
    assert_eq!(
        client
            .post("/post/renew-timestamp-token")
            .json(&json!({"token": before.token}))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
}

async fn check_single(client: &Client, public: bool) {
    let body = json!({"hash": PRIVATE, "audience": "public"});
    let before = snapshot(client, true, "descending").await;
    let direct_before = resource(client, PRIVATE, true).await;
    assert_eq!(direct_before["data"]["audience"], "private");
    let direct_before = serde_json::from_value(direct_before).unwrap();
    assert_eq!(change(client, body, true).await, Status::Ok);
    assert_eq!(
        resource(client, PRIVATE, true).await["data"]["audience"],
        "public"
    );
    check_owner_snapshots(client, &before, &direct_before, "public").await;
    if public {
        assert!(
            resource(client, PRIVATE, false).await["data"]
                .get("audience")
                .is_none()
        );
        let current = snapshot(client, false, "descending").await;
        assert!(hashes(client, &current, false).await.contains(PRIVATE));
    }
    let token = ClaimsHash::new(id(PRIVATE), 0, false).encode();
    let media = format!("/object/thumbnail/cc/{PRIVATE}-v7.jpg?token={token}");
    if public {
        let response = client.get(&media).dispatch().await;
        assert_eq!(response.status(), Status::Ok);
        assert_eq!(
            response.headers().get_one("Cache-Control"),
            Some("no-store")
        );
    }
    assert_eq!(
        change(
            client,
            json!({"hash": PRIVATE, "audience": "private"}),
            true
        )
        .await,
        Status::Ok
    );
    assert_eq!(
        resource(client, PRIVATE, true).await["data"]["audience"],
        "private"
    );
    check_owner_snapshots(client, &before, &direct_before, "private").await;
    if public {
        assert_eq!(
            client.get(&media).dispatch().await.status(),
            Status::NotFound
        );
        assert_eq!(
            client
                .get(format!("/get/resource/{PRIVATE}"))
                .dispatch()
                .await
                .status(),
            Status::NotFound
        );
        let current = snapshot(client, false, "descending").await;
        assert!(!hashes(client, &current, false).await.contains(PRIVATE));
    }
    for body in [
        json!({"hash": ALBUM, "audience": "public"}),
        json!({"hash": PRIVATE, "audience": "public", "timestamp": 1}),
        json!({"hash": "bad", "audience": "public"}),
    ] {
        assert_eq!(change(client, body, true).await, Status::BadRequest);
    }
    assert_eq!(
        change(
            client,
            json!({"hash": "0".repeat(64), "audience": "public"}),
            true
        )
        .await,
        Status::NotFound
    );
}

async fn check_batch(client: &Client) {
    let current = snapshot(client, true, "descending").await;
    let response = owner(client.get(snapshot_path("data", &current)), true)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let data = response.into_json::<Vec<Value>>().await.unwrap();
    let index = |hash: &str| {
        data.iter()
            .position(|value| value["abstractData"]["id"] == hash)
            .unwrap()
    };
    let body = json!({"timestamp": current.prefetch.timestamp, "selection": {"mode": "explicit", "indices": [index(PRIVATE), index(MISSING)]}, "audience": "public"});
    assert_eq!(
        change(client, body.clone(), false).await,
        Status::Unauthorized
    );
    let revision = TREE.store.photo_audiences().unwrap().revision;
    assert_eq!(change(client, json!({"timestamp": current.prefetch.timestamp, "selection": {"mode": "explicit", "indices": [index(PRIVATE), index(ALBUM)]}, "audience": "public"}), true).await, Status::BadRequest);
    assert_eq!(TREE.store.photo_audiences().unwrap().revision, revision);
    assert_eq!(change(client, body, true).await, Status::Ok);
    assert_eq!(TREE.store.photo_audiences().unwrap().revision, revision + 1);
    for hash in [PRIVATE, MISSING] {
        assert_eq!(
            resource(client, hash, true).await["data"]["audience"],
            "public"
        );
    }
    // The second batch reuses the original indices and snapshot after publication.
    let response = owner(client.get(snapshot_path("data", &current)), true)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let data = response.into_json::<Vec<Value>>().await.unwrap();
    let excluded = data
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            (!matches!(
                value["abstractData"]["id"].as_str(),
                Some(PRIVATE | MISSING)
            ))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(change(client, json!({"timestamp": current.prefetch.timestamp, "selection": {"mode": "allExcept", "excludedIndices": excluded}, "audience": "private"}), true).await, Status::Ok);
    for hash in [PRIVATE, MISSING] {
        assert_eq!(
            resource(client, hash, true).await["data"]["audience"],
            "private"
        );
    }
    assert_eq!(change(client, json!({"timestamp": -1, "selection": {"mode": "explicit", "indices": [0]}, "audience": "public"}), true).await, Status::Conflict);
}

fn image_upload(pixel: u8) -> (String, Vec<u8>) {
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        32,
        24,
        image::Rgb([pixel, 30, 90]),
    ))
    .write_to(&mut image, image::ImageFormat::Png)
    .unwrap();
    let bytes = image.into_inner();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let mut body = b"--fixture-upload\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fixture.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    body.extend(bytes);
    body.extend(b"\r\n--fixture-upload\r\nContent-Disposition: form-data; name=\"lastModified\"\r\n\r\n0\r\n--fixture-upload--\r\n");
    (hash, body)
}

async fn check_upload(client: &Client, public: bool) {
    for (pixel, query, expected) in [
        (1, "", "private"),
        (2, "?publish_immediately=false", "private"),
        (3, "?publish_immediately=true", "public"),
    ] {
        let (hash, body) = image_upload(pixel);
        let url = format!("/upload{query}");
        let request = || {
            client
                .post(&url)
                .header(Header::new(
                    "Content-Type",
                    "multipart/form-data; boundary=fixture-upload",
                ))
                .body(body.clone())
        };
        assert_eq!(request().dispatch().await.status(), Status::Unauthorized);
        let response = owner(request(), true).dispatch().await;
        assert_eq!(
            response.status(),
            Status::Ok,
            "{}",
            response.into_string().await.unwrap_or_default()
        );
        assert_eq!(
            resource(client, &hash, true).await["data"]["audience"],
            expected
        );
        if public {
            assert_eq!(
                client
                    .get(format!("/get/resource/{hash}"))
                    .dispatch()
                    .await
                    .status(),
                if expected == "public" {
                    Status::Ok
                } else {
                    Status::NotFound
                }
            );
        }
    }
    // A later upload with the default choice preserves an existing public duplicate.
    let (hash, body) = image_upload(3);
    assert_eq!(
        owner(
            client
                .post("/upload")
                .header(Header::new(
                    "Content-Type",
                    "multipart/form-data; boundary=fixture-upload"
                ))
                .body(body),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert_eq!(
        resource(client, &hash, true).await["data"]["audience"],
        "public"
    );
    // A share may upload private photos only, even with a forged publication query.
    let (hash, body) = image_upload(4);
    for publish in [true, false] {
        let response = client
            .post(format!(
                "/upload?presigned_album_id_opt={ALBUM}&publish_immediately={publish}"
            ))
            .header(Header::new(
                "Content-Type",
                "multipart/form-data; boundary=fixture-upload",
            ))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share"))
            .body(body.clone())
            .dispatch()
            .await;
        assert_eq!(
            response.status(),
            if public || publish {
                Status::Unauthorized
            } else {
                Status::Ok
            }
        );
    }
    if !public {
        assert_eq!(
            resource(client, &hash, true).await["data"]["audience"],
            "private"
        );
    }
}

pub(super) async fn check(client: &Client) {
    let public = crate::router::fairing::guard_gallery::public_gallery_enabled();
    check_mutation_guards(client).await;
    check_single(client, public).await;
    check_batch(client).await;
    check_upload(client, public).await;
}
