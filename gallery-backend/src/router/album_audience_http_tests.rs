//! Album privacy through actual HTTP routes, using only disposable fixture storage.
use std::{
    collections::HashSet,
    process::{Command, Stdio},
    sync::RwLock,
    time::{Duration, Instant},
};

use arrayvec::ArrayString;
use rocket::{
    http::{ContentType, Cookie, Status},
    local::asynchronous::{Client, LocalRequest},
};
use serde_json::{Value, json};

use crate::{
    public::{
        constant::storage::{DATA_PATH, get_data_path},
        db::{
            tree::{TREE, state::TreeState},
            write_behind::WRITE_BEHIND,
        },
        structure::{
            abstract_data::AbstractData,
            album::Album,
            audience::Audience,
            common::FileModify,
            config::{APP_CONFIG, AppConfig},
            expression::{AlbumFilterValue, Expression},
            image::{ImageCombined, ImageMetadata},
            object::{ObjectSchema, ObjectType},
        },
    },
    router::{
        claims::Claims,
        fairing::guard_timestamp::renew_timestamp_token,
        get::{
            get_data::{get_data, get_rows, get_scroll_bar},
            get_img::small_thumbnail_file,
            get_list::get_albums,
            get_prefetch::{PrefetchReturn, prefetch},
            get_resource::get_resource,
        },
        post::create_album::create_empty_album,
        put::{album_audience::set_album_audience, edit_album::edit_album},
    },
    storage::DataStore,
};

const KEY: &[u8] = b"album-audience-fixture-signing-key-only";
const PUBLIC_ALBUM: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PRIVATE_ONLY_ALBUM: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PRIVATE_ALBUM: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PUBLIC_OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PUBLIC_NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PRIVATE_COVER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const PUBLIC_OUTSIDE: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PRIVATE_ONLY: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const TRASHED: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const LATER: &str = "9999999999999999999999999999999999999999999999999999999999999999";

fn id(value: &str) -> ArrayString<64> {
    ArrayString::from(value).unwrap()
}

fn photo(hash: &str, album: &str, timestamp: i64, thumbhash: u8) -> AbstractData {
    let mut metadata = ImageMetadata::new(id(hash), 13, 32, 24, "jpg".into());
    metadata.albums.insert(id(album));
    metadata.alias.push(FileModify {
        file: format!("/fixture/{hash}.jpg"),
        modified: timestamp,
        scan_time: timestamp,
    });
    let mut record = AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id(hash), ObjectType::Image),
        metadata,
    });
    record.set_thumbhash(vec![thumbhash]);
    record.set_cache_version(7);
    record
}

fn seed() {
    let store = DataStore::initialize_empty(&get_data_path().join("db/index_v6.redb")).unwrap();
    // Simulate the integrated photo migration already having completed. The album
    // migration must never give these later private photos a public grant.
    store.migrate_photo_audience().unwrap();
    let mut album = Album::new(id(PUBLIC_ALBUM), Some("Public album".into()));
    album.cover = Some(id(PRIVATE_COVER));
    album.thumbhash = Some(vec![3]);
    album.item_count = 3;
    album.item_size = 39;
    let mut private_only = Album::new(id(PRIVATE_ONLY_ALBUM), Some("Empty public view".into()));
    private_only.cover = Some(id(PRIVATE_ONLY));
    private_only.thumbhash = Some(vec![5]);
    private_only.item_count = 1;
    private_only.item_size = 13;
    let mut trashed = photo(TRASHED, PUBLIC_ALBUM, 4_000, 6);
    trashed.set_trashed(true);
    let mut records = vec![
        album.into_abstract_data(),
        private_only.into_abstract_data(),
        photo(PUBLIC_OLD, PUBLIC_ALBUM, 1_000, 1),
        photo(PUBLIC_NEW, PUBLIC_ALBUM, 2_000, 2),
        photo(PRIVATE_COVER, PUBLIC_ALBUM, 3_000, 3),
        photo(PRIVATE_ONLY, PRIVATE_ONLY_ALBUM, 5_000, 5),
        trashed,
    ];
    store
        .write(|writer| -> anyhow::Result<()> {
            for record in &records {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    store.migrate_album_audience().unwrap();
    let added = vec![
        Album::new(id(PRIVATE_ALBUM), Some("secret-album-name-marker".into())).into_abstract_data(),
        photo(PUBLIC_OUTSIDE, PRIVATE_ALBUM, 6_000, 7),
        photo(LATER, PRIVATE_ALBUM, 7_000, 8),
    ];
    store
        .write(|writer| -> anyhow::Result<()> {
            for record in &added {
                writer.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    for hash in [PUBLIC_OLD, PUBLIC_NEW, PUBLIC_OUTSIDE, TRASHED] {
        store.set_photo_audience(hash, Audience::Public).unwrap();
    }
    // The independent marker also leaves albums created after it private.
    store.migrate_album_audience().unwrap();
    assert_eq!(
        store.photo_audiences().unwrap().get(PRIVATE_COVER).unwrap(),
        Audience::Private
    );
    assert_eq!(
        store.photo_audiences().unwrap().get(PRIVATE_ALBUM).unwrap(),
        Audience::Private
    );
    drop(store);
    records.extend(added);
    for record in records
        .iter()
        .filter(|record| !matches!(record, AbstractData::Album(_)))
    {
        let path = record.small_thumbnail_path_for_version(7);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"synthetic-album-thumbnail").unwrap();
    }
    *TREE.state.write().unwrap() = TreeState::from_records(records);
}

fn owner(request: LocalRequest<'_>, is_owner: bool) -> LocalRequest<'_> {
    if is_owner {
        request.cookie(Cookie::new("jwt", Claims::new_admin().encode_with_key(KEY)))
    } else {
        request
    }
}

async fn resource(client: &Client, hash: &str, is_owner: bool) -> Value {
    let response = owner(client.get(format!("/get/resource/{hash}")), is_owner)
        .dispatch()
        .await;
    assert_eq!(
        response.status(),
        Status::Ok,
        "resource {hash}, owner={is_owner}"
    );
    response.into_json().await.unwrap()
}

async fn snapshot(
    client: &Client,
    query: &Expression,
    is_owner: bool,
    sort: &str,
) -> PrefetchReturn {
    let response = owner(
        client
            .post(format!("/get/prefetch?sort={sort}"))
            .header(ContentType::JSON)
            .body(serde_json::to_string(query).unwrap()),
        is_owner,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

fn snapshot_path(route: &str, snapshot: &PrefetchReturn) -> String {
    let timestamp = snapshot.prefetch.timestamp;
    let token = &snapshot.token;
    match route {
        "data" => format!("/get/get-data?timestamp={timestamp}&start=0&end=100&token={token}"),
        "rows" => format!("/get/get-rows?timestamp={timestamp}&index=0&token={token}"),
        "scroll" => format!("/get/get-scroll-bar?timestamp={timestamp}&token={token}"),
        _ => unreachable!(),
    }
}

async fn data(client: &Client, snapshot: &PrefetchReturn, is_owner: bool) -> Vec<Value> {
    let response = owner(client.get(snapshot_path("data", snapshot)), is_owner)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

fn hashes(records: &[Value]) -> HashSet<&str> {
    records
        .iter()
        .map(|record| record["abstractData"]["id"].as_str().unwrap())
        .collect()
}

async fn members(client: &Client, album: &str, is_owner: bool) -> Vec<Value> {
    let query = Expression::And(vec![
        Expression::Trashed(false),
        Expression::Album(AlbumFilterValue::Value(id(album))),
    ]);
    let snapshot = snapshot(client, &query, is_owner, "descending").await;
    data(client, &snapshot, is_owner).await
}

async fn publish(client: &Client, album: &str, audience: &str, publish_photos: Option<bool>) {
    let mut body = json!({"albumId": album, "audience": audience});
    if let Some(publish_photos) = publish_photos {
        body["publishPhotos"] = publish_photos.into();
    }
    let response = owner(
        client
            .put("/put/album_audience")
            .header(ContentType::JSON)
            .body(body.to_string()),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    assert!(response.into_string().await.unwrap_or_default().is_empty());
}

async fn stale_snapshot(client: &Client, previous: &PrefetchReturn, is_owner: bool) {
    for route in ["data", "rows", "scroll"] {
        assert_eq!(
            owner(client.get(snapshot_path(route, previous)), is_owner)
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
                .header(ContentType::JSON)
                .body(json!({"token": previous.token}).to_string()),
            is_owner
        )
        .dispatch()
        .await
        .status(),
        Status::Unauthorized
    );
}

#[allow(clippy::too_many_lines)]
async fn read_checks(client: &Client) {
    for is_owner in [true, false] {
        let response = owner(client.get("/get/get-albums"), is_owner)
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Ok);
        let albums: Vec<Value> = response.into_json().await.unwrap();
        assert_eq!(albums.len(), if is_owner { 3 } else { 2 });
        for album in &albums {
            assert_eq!(
                album["audience"],
                if album["albumId"] == PRIVATE_ALBUM {
                    "private"
                } else {
                    "public"
                }
            );
        }
        if !is_owner {
            assert!(
                !serde_json::to_string(&albums)
                    .unwrap()
                    .contains("secret-album-name-marker")
            );
        }
    }
    assert_eq!(
        client
            .get(format!("/get/resource/{PRIVATE_ALBUM}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    assert_eq!(
        resource(client, PRIVATE_ALBUM, true).await["data"]["abstractData"]["audience"],
        "private"
    );
    let anonymous = resource(client, PUBLIC_ALBUM, false).await;
    let album = &anonymous["data"]["abstractData"];
    assert_eq!(album["audience"], "public");
    assert_eq!(album["itemCount"], 2);
    assert_eq!(album["itemSize"], 26);
    assert_eq!(album["cover"], PUBLIC_NEW);
    assert_eq!(album["thumbhash"], json!([2]));
    assert_eq!(album["cacheVersion"], 7);
    let owned = resource(client, PUBLIC_ALBUM, true).await;
    assert_eq!(owned["data"]["abstractData"]["cover"], PRIVATE_COVER);
    assert_eq!(owned["data"]["abstractData"]["thumbhash"], json!([3]));
    let token = anonymous["data"]["token"].as_str().unwrap();
    assert_eq!(
        client
            .get(format!(
                "/object/thumbnail/bb/{PUBLIC_NEW}-v7.jpg?token={token}"
            ))
            .dispatch()
            .await
            .status(),
        Status::Ok
    );
    assert_eq!(
        client
            .get(format!(
                "/object/thumbnail/cc/{PRIVATE_COVER}-v7.jpg?token={token}"
            ))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    let empty = resource(client, PRIVATE_ONLY_ALBUM, false).await;
    assert_eq!(empty["data"]["abstractData"]["itemCount"], 0);
    assert_eq!(empty["data"]["abstractData"]["itemSize"], 0);
    assert!(empty["data"]["abstractData"]["cover"].is_null());
    assert!(empty["data"]["abstractData"]["thumbhash"].is_null());
    assert_eq!(empty["data"]["token"], "");
    assert_eq!(
        resource(client, PRIVATE_ONLY_ALBUM, true).await["data"]["abstractData"]["cover"],
        PRIVATE_ONLY
    );
    for sort in ["descending", "ascending", "random"] {
        let query = Expression::Trashed(false);
        let owner_snapshot = snapshot(client, &query, true, sort).await;
        assert!(hashes(&data(client, &owner_snapshot, true).await).contains(PRIVATE_ALBUM));
        let public_snapshot = snapshot(client, &query, false, sort).await;
        let records = data(client, &public_snapshot, false).await;
        assert_eq!(
            hashes(&records),
            HashSet::from([
                PUBLIC_ALBUM,
                PRIVATE_ONLY_ALBUM,
                PUBLIC_OLD,
                PUBLIC_NEW,
                PUBLIC_OUTSIDE
            ])
        );
        let listed_album = &records
            .iter()
            .find(|record| record["abstractData"]["id"] == PUBLIC_ALBUM)
            .unwrap()["abstractData"];
        assert_eq!(listed_album, album);
    }
    // Prime owner album-ID filtering before anonymous use of identical queries.
    assert_eq!(
        hashes(&members(client, PRIVATE_ALBUM, true).await),
        HashSet::from([PUBLIC_OUTSIDE, LATER])
    );
    assert!(members(client, PRIVATE_ALBUM, false).await.is_empty());
    let outside = resource(client, PUBLIC_OUTSIDE, false).await;
    assert_eq!(outside["data"]["abstractData"]["id"], PUBLIC_OUTSIDE);
    assert_eq!(outside["data"]["abstractData"]["albums"], json!([]));
    assert_eq!(
        hashes(&members(client, PUBLIC_ALBUM, false).await),
        HashSet::from([PUBLIC_OLD, PUBLIC_NEW])
    );
    let name = Expression::Any("secret-album-name-marker".into());
    let result = snapshot(client, &name, false, "descending").await;
    assert!(data(client, &result, false).await.is_empty());
    for (filter, expected) in [
        (
            Expression::Album(AlbumFilterValue::Exists(true)),
            HashSet::from([PUBLIC_OLD, PUBLIC_NEW]),
        ),
        (
            Expression::Album(AlbumFilterValue::Exists(false)),
            HashSet::from([PUBLIC_ALBUM, PRIVATE_ONLY_ALBUM, PUBLIC_OUTSIDE]),
        ),
        (
            Expression::Not(Box::new(Expression::Album(AlbumFilterValue::Value(id(
                PRIVATE_ALBUM,
            ))))),
            HashSet::from([
                PUBLIC_ALBUM,
                PRIVATE_ONLY_ALBUM,
                PUBLIC_OLD,
                PUBLIC_NEW,
                PUBLIC_OUTSIDE,
            ]),
        ),
    ] {
        let query = Expression::And(vec![Expression::Trashed(false), filter]);
        let _ = snapshot(client, &query, true, "descending").await;
        let result = snapshot(client, &query, false, "descending").await;
        assert_eq!(hashes(&data(client, &result, false).await), expected);
    }
}

#[allow(clippy::too_many_lines)]
async fn authorization_checks(client: &Client) {
    let body =
        json!({"albumId": PRIVATE_ALBUM, "audience": "public", "publishPhotos": true}).to_string();
    let response = client
        .put("/put/album_audience")
        .header(ContentType::JSON)
        .body(body.clone())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    let mut public = Claims::new_public();
    public.exp = Claims::new_admin().exp;
    assert_eq!(
        client
            .put("/put/album_audience")
            .header(ContentType::JSON)
            .body(body.clone())
            .cookie(Cookie::new("jwt", public.encode_with_key(KEY)))
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
                .put("/put/album_audience")
                .header(ContentType::JSON)
                .body(body),
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
    assert_eq!(
        TREE.store
            .photo_audiences()
            .unwrap()
            .get(PRIVATE_ALBUM)
            .unwrap(),
        Audience::Private
    );
    assert_eq!(
        TREE.store.photo_audiences().unwrap().get(LATER).unwrap(),
        Audience::Private
    );
    let invalid = json!({"albumId": PRIVATE_ALBUM, "audience": "private", "publishPhotos": true});
    assert_eq!(
        owner(
            client
                .put("/put/album_audience")
                .header(ContentType::JSON)
                .body(invalid.to_string()),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::BadRequest
    );
    for target in [
        "7777777777777777777777777777777777777777777777777777777777777777",
        PUBLIC_NEW,
    ] {
        let body = json!({"albumId": target, "audience": "public"});
        assert_eq!(
            owner(
                client
                    .put("/put/album_audience")
                    .header(ContentType::JSON)
                    .body(body.to_string()),
                true,
            )
            .dispatch()
            .await
            .status(),
            Status::NotFound
        );
    }
}

async fn add_member(client: &Client, album: &str, hash: &str) {
    let item = resource(client, hash, true).await;
    let body = json!({
        "indexArray": [0], "addAlbumsArray": [album], "removeAlbumsArray": [],
        "timestamp": item["prefetch"]["timestamp"],
    });
    assert_eq!(
        owner(
            client
                .put("/put/edit_album")
                .header(ContentType::JSON)
                .body(body.to_string()),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert!(WRITE_BEHIND.status().pending_operations > 0);
}

#[allow(clippy::too_many_lines)]
async fn publication_checks(client: &Client) {
    let created = owner(client.post("/post/create_empty_album"), true)
        .dispatch()
        .await;
    assert_eq!(created.status(), Status::Ok);
    let created = created.into_string().await.unwrap();
    assert_eq!(
        resource(client, &created, true).await["data"]["abstractData"]["audience"],
        "private"
    );
    assert_eq!(
        client
            .get(format!("/get/resource/{created}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    add_member(client, &created, PRIVATE_ONLY).await;
    let previous = snapshot(client, &Expression::Trashed(false), false, "descending").await;
    let worker = tokio::spawn(WRITE_BEHIND.run_worker());
    publish(client, &created, "public", None).await;
    stale_snapshot(client, &previous, false).await;
    assert_eq!(
        resource(client, &created, false).await["data"]["abstractData"]["itemCount"],
        0
    );
    assert_eq!(
        client
            .get(format!("/get/resource/{PRIVATE_ONLY}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    assert_eq!(
        TREE.store
            .photo_audiences()
            .unwrap()
            .get(PRIVATE_ONLY)
            .unwrap(),
        Audience::Private
    );

    // This member exists only in the pending overlay when publication begins.
    add_member(client, &created, PRIVATE_COVER).await;
    let previous = snapshot(client, &Expression::Trashed(false), true, "descending").await;
    publish(client, &created, "public", Some(true)).await;
    stale_snapshot(client, &previous, true).await;
    assert_eq!(
        hashes(&members(client, &created, false).await),
        HashSet::from([PRIVATE_ONLY, PRIVATE_COVER])
    );
    assert_eq!(WRITE_BEHIND.status().pending_operations, 0);

    add_member(client, &created, LATER).await;
    assert!(WRITE_BEHIND.flush(Duration::from_secs(5)).await);
    assert_eq!(
        TREE.store.photo_audiences().unwrap().get(LATER).unwrap(),
        Audience::Private
    );
    assert_eq!(
        resource(client, &created, false).await["data"]["abstractData"]["itemCount"],
        2
    );
    assert_eq!(
        hashes(&members(client, &created, false).await),
        HashSet::from([PRIVATE_ONLY, PRIVATE_COVER])
    );
    let previous = snapshot(client, &Expression::Trashed(false), false, "descending").await;
    publish(client, &created, "private", Some(false)).await;
    stale_snapshot(client, &previous, false).await;
    assert_eq!(
        client
            .get(format!("/get/resource/{created}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    assert!(members(client, &created, false).await.is_empty());
    for hash in [PRIVATE_ONLY, PRIVATE_COVER] {
        assert_eq!(
            TREE.store.photo_audiences().unwrap().get(hash).unwrap(),
            Audience::Public
        );
        assert_eq!(
            resource(client, hash, false).await["data"]["abstractData"]["id"],
            hash
        );
    }
    // Trashed photos must not acquire publication grants through the bulk action.
    TREE.store
        .set_photo_audience(TRASHED, Audience::Private)
        .unwrap();
    publish(client, PUBLIC_ALBUM, "public", Some(true)).await;
    assert_eq!(
        TREE.store.photo_audiences().unwrap().get(TRASHED).unwrap(),
        Audience::Private
    );
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
}

#[rocket::async_test]
async fn http_child() {
    let Some(root) = std::env::var_os("UROCISSA_ALBUM_AUDIENCE_TEST_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("fixture-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(KEY.to_vec()).unwrap());
    config.public.write_behind.flush_interval_ms = 60_000;
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    seed();
    let client = Client::untracked(
        rocket::custom(rocket::Config {
            port: 0,
            log_level: rocket::config::LogLevel::Off,
            ..rocket::Config::debug_default()
        })
        .mount(
            "/",
            routes![
                prefetch,
                get_data,
                get_rows,
                get_scroll_bar,
                get_resource,
                get_albums,
                small_thumbnail_file,
                renew_timestamp_token,
                set_album_audience,
                create_empty_album,
                edit_album,
            ],
        ),
    )
    .await
    .unwrap();
    read_checks(&client).await;
    authorization_checks(&client).await;
    publication_checks(&client).await;
}

#[test]
fn album_audience_http() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_ALBUM_AUDIENCE_TEST_ROOT", root.path())
        .env("UROCISSA_PUBLIC_GALLERY", "1")
        .args([
            "--exact",
            "router::album_audience_http_tests::http_child",
            "--nocapture",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Album audience HTTP fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
