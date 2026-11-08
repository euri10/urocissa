//! Actual read routes and guards, isolated from all real configuration, media and databases.
use std::{
    collections::HashSet,
    process::{Command, Stdio},
    sync::RwLock,
    time::{Duration, Instant},
};

use arrayvec::ArrayString;
use chrono::{TimeZone, Utc};
use rocket::{
    http::{ContentType, Cookie, Header, Status},
    local::asynchronous::{Client, LocalRequest},
};
use serde_json::Value;

mod auto_tags;
mod media;
mod mutations;
mod search;
mod tag_feedback;

use crate::{
    public::{
        constant::storage::DATA_PATH,
        db::tree::{TREE, state::TreeState},
        structure::{
            abstract_data::AbstractData,
            album::{Album, Share},
            audience::Audience,
            common::FileModify,
            config::{APP_CONFIG, AppConfig},
            expression::Expression,
            image::{ImageCombined, ImageMetadata},
            object::{ObjectSchema, ObjectType},
            video::{VideoCombined, VideoMetadata},
        },
    },
    router::{
        claims::{Claims, claims_hash::ClaimsHash},
        fairing::{
            cache_control_fairing::cache_control_fairing, guard_hash::renew_hash_token,
            guard_timestamp::renew_timestamp_token,
        },
        get::{
            get_data::{get_data, get_rows, get_scroll_bar},
            get_img::{compressed_file, imported_file, small_thumbnail_file},
            get_list::{get_albums, get_search_facets},
            get_prefetch::{PrefetchReturn, prefetch},
            get_resource::get_resource,
        },
        post::post_upload::upload,
        put::{edit_photo_audience::edit_photo_audience, edit_share::edit_share},
    },
    storage::DataStore,
};

const KEY: &[u8] = b"audience-fixture-signing-key-only";
const PUBLIC: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PUBLIC_VIDEO: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PRIVATE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const MISSING: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PRIVATE_VIDEO: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const TRASHED: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const ALBUM: &str = "1111111111111111111111111111111111111111111111111111111111111111";

mod aggregates;

fn id(value: &str) -> ArrayString<64> {
    ArrayString::from(value).unwrap()
}

fn photo(hash: &str, video: bool, shared: bool) -> AbstractData {
    let mut data = if video {
        let mut metadata = VideoMetadata::new(id(hash), 13, 32, 24, "mp4".into());
        if shared {
            metadata.albums.insert(id(ALBUM));
        }
        AbstractData::Video(VideoCombined {
            object: ObjectSchema::new(id(hash), ObjectType::Video),
            metadata,
        })
    } else {
        let mut metadata = ImageMetadata::new(id(hash), 13, 32, 24, "jpg".into());
        if shared {
            metadata.albums.insert(id(ALBUM));
        }
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id(hash), ObjectType::Image),
            metadata,
        })
    };
    let (month, tag, make, model) = match hash {
        PUBLIC => (1, "public-search-tag", "Canon", "Visible model"),
        PUBLIC_VIDEO => (2, "outside-share-tag", "Canon", "Visible model"),
        PRIVATE => (3, "private-search-tag", "Private make", "Private model"),
        MISSING => (4, "unshared-private-tag", "CANON", "Unshared model"),
        PRIVATE_VIDEO => (5, "private-video-tag", "CANON", "Private model"),
        TRASHED => (6, "trash-tag", "Trash make", "Trash model"),
        _ => unreachable!(),
    };
    let timestamp = Utc
        .with_ymd_and_hms(2024, month, 15, 12, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    data.alias_mut().unwrap().push(FileModify {
        file: format!("/private-path-marker/{hash}"),
        modified: timestamp,
        scan_time: timestamp,
    });
    data.exif_vec_mut()
        .unwrap()
        .insert("GPSLatitude".into(), "private-gps-marker".into());
    data.exif_vec_mut()
        .unwrap()
        .insert("Make".into(), make.into());
    data.exif_vec_mut()
        .unwrap()
        .insert("Model".into(), model.into());
    data.tag_mut().insert(tag.into());
    data.tag_mut().insert("common-tag".into());
    data.set_cache_version(7);
    data
}

fn seed() -> Vec<AbstractData> {
    let mut album = Album::new(id(ALBUM), Some("Fixture album".into()));
    album.item_count = 3;
    album.item_size = 39;
    album.share_list.insert(
        id("fixture-share"),
        Share {
            show_metadata: true,
            show_upload: true,
            ..Share::default()
        },
    );
    let mut trashed = photo(TRASHED, false, true);
    trashed.set_trashed(true);
    let mut records = vec![
        photo(PUBLIC, false, true),
        photo(PUBLIC_VIDEO, true, false),
        trashed,
        album.into_abstract_data(),
    ];
    let store = DataStore::initialize_empty(
        &crate::public::constant::storage::get_data_path().join("db/index_v6.redb"),
    )
    .unwrap();
    store
        .write(|writer| -> anyhow::Result<()> {
            for data in &records {
                writer.insert(data)?;
            }
            Ok(())
        })
        .unwrap();
    store.migrate_photo_audience().unwrap();
    store.migrate_album_audience().unwrap();
    let new = vec![
        photo(PRIVATE, false, true),
        photo(MISSING, false, false),
        photo(PRIVATE_VIDEO, true, true),
    ];
    store
        .write(|writer| -> anyhow::Result<()> {
            for data in &new {
                writer.insert(data)?;
            }
            Ok(())
        })
        .unwrap();
    store
        .set_photo_audience(PRIVATE, Audience::Private)
        .unwrap();
    store.migrate_photo_audience().unwrap();
    drop(store);
    records.extend(new);
    for data in &records {
        if matches!(data, AbstractData::Album(_)) {
            continue;
        }
        for path in [
            data.imported_path(),
            data.compressed_path(),
            data.thumbnail_path_for_version(7),
            data.small_thumbnail_path_for_version(7),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture-media").unwrap();
        }
    }
    *TREE.state.write().unwrap() = TreeState::from_records(records.clone());
    records
}

fn owner(request: LocalRequest<'_>, enabled: bool) -> LocalRequest<'_> {
    if enabled {
        request.cookie(Cookie::new("jwt", Claims::new_admin().encode_with_key(KEY)))
    } else {
        request
    }
}

async fn snapshot(client: &Client, is_owner: bool, sort: &str) -> PrefetchReturn {
    // Equal query content deliberately exercises the owner/anonymous cache boundary.
    let request = client
        .post(format!("/get/prefetch?sort={sort}&locate={MISSING}"))
        .header(ContentType::JSON)
        .body(serde_json::to_string(&Expression::Trashed(false)).unwrap());
    let response = owner(request, is_owner).dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    response.into_json().await.unwrap()
}

fn snapshot_path(route: &str, snapshot: &PrefetchReturn) -> String {
    let timestamp = snapshot.prefetch.timestamp;
    match route {
        "data" => format!(
            "/get/get-data?timestamp={timestamp}&start=0&end=100&token={}",
            snapshot.token
        ),
        "rows" => format!(
            "/get/get-rows?timestamp={timestamp}&index=0&token={}",
            snapshot.token
        ),
        "scroll" => format!(
            "/get/get-scroll-bar?timestamp={timestamp}&token={}",
            snapshot.token
        ),
        _ => unreachable!(),
    }
}

async fn hashes(client: &Client, snapshot: &PrefetchReturn, is_owner: bool) -> HashSet<String> {
    let response = owner(client.get(snapshot_path("data", snapshot)), is_owner)
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    response
        .into_json::<Vec<Value>>()
        .await
        .unwrap()
        .into_iter()
        .map(|data| {
            let record = &data["abstractData"];
            if record["type"] != "album" {
                if is_owner {
                    assert!(matches!(
                        data["audience"].as_str(),
                        Some("public" | "private")
                    ));
                    assert_eq!(record["exifVec"]["GPSLatitude"], "private-gps-marker");
                    assert!(record["alias"].is_array());
                } else {
                    assert!(data.get("audience").is_none());
                    assert!(!record.to_string().contains("private-gps-marker"));
                    assert!(record.get("alias").is_none());
                }
            }
            if record["type"] == "album" {
                assert!(data.get("audience").is_none());
            }
            record["id"].as_str().unwrap().to_owned()
        })
        .collect()
}

async fn search_privacy_checks(client: &Client) {
    for term in [
        "private-path-marker",
        "public-search-tag",
        "private-search-tag",
    ] {
        let query = Expression::And(vec![
            Expression::Trashed(false),
            Expression::Any(term.into()),
        ]);
        // Prime the owner cache first, then reuse exactly the same public query.
        for is_owner in [true, false, true, false] {
            let response = owner(
                client
                    .post("/get/prefetch")
                    .header(ContentType::JSON)
                    .body(serde_json::to_string(&query).unwrap()),
                is_owner,
            )
            .dispatch()
            .await;
            assert_eq!(response.status(), Status::Ok);
            let snapshot = response.into_json::<PrefetchReturn>().await.unwrap();
            let expected = match (term, is_owner) {
                ("private-path-marker", true) => {
                    vec![PUBLIC, PUBLIC_VIDEO, PRIVATE, MISSING, PRIVATE_VIDEO]
                }
                ("public-search-tag", _) => vec![PUBLIC],
                ("private-search-tag", true) => vec![PRIVATE],
                _ => vec![],
            }
            .into_iter()
            .map(str::to_owned)
            .collect();
            assert_eq!(hashes(client, &snapshot, is_owner).await, expected);
        }
    }
    let query = Expression::Not(Box::new(Expression::Path("private-path-marker".into())));
    let response = client
        .post("/get/prefetch")
        .header(ContentType::JSON)
        .body(serde_json::to_string(&query).unwrap())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::BadRequest);
    assert!(
        response
            .into_string()
            .await
            .unwrap()
            .contains("Path expressions are unavailable")
    );
}

#[allow(clippy::too_many_lines)]
async fn public_checks(client: &Client, records: &[AbstractData]) {
    search_privacy_checks(client).await;
    aggregates::checks(client, false).await;
    let expected = HashSet::from([PUBLIC.to_owned(), PUBLIC_VIDEO.to_owned(), ALBUM.to_owned()]);
    for sort in ["descending", "ascending", "random"] {
        let owner_snapshot = snapshot(client, true, sort).await;
        assert_eq!(
            hashes(client, &owner_snapshot, true).await.len(),
            records.len() - 1
        );
        let public_snapshot = snapshot(client, false, sort).await;
        assert_eq!(public_snapshot.prefetch.data_length, expected.len());
        assert_eq!(public_snapshot.prefetch.locate_to, None);
        assert_eq!(hashes(client, &public_snapshot, false).await, expected);
        for route in ["rows", "scroll"] {
            assert_eq!(
                client
                    .get(snapshot_path(route, &public_snapshot))
                    .dispatch()
                    .await
                    .status(),
                Status::Ok
            );
        }
        // Owner snapshots cannot become anonymous grants through their bearer tokens.
        for route in ["data", "rows", "scroll"] {
            assert_eq!(
                client
                    .get(snapshot_path(route, &owner_snapshot))
                    .dispatch()
                    .await
                    .status(),
                Status::Unauthorized
            );
        }
    }
    for hash in [PRIVATE, MISSING, PRIVATE_VIDEO, TRASHED] {
        assert_eq!(
            client
                .get(format!("/get/resource/{hash}"))
                .dispatch()
                .await
                .status(),
            Status::NotFound
        );
        assert_eq!(
            owner(client.get(format!("/get/resource/{hash}")), true)
                .dispatch()
                .await
                .status(),
            Status::Ok
        );
    }
    assert_eq!(
        client
            .get(format!("/get/resource/{PUBLIC}"))
            .dispatch()
            .await
            .status(),
        Status::Ok
    );

    for data in records
        .iter()
        .filter(|data| !matches!(data, AbstractData::Album(_)))
    {
        let hash = data.hash();
        let token = ClaimsHash::new(hash, 0, true).encode();
        for path in [
            data.imported_path(),
            data.compressed_path(),
            data.thumbnail_path_for_version(7),
            data.small_thumbnail_path_for_version(7),
        ] {
            let path = path
                .strip_prefix(crate::public::constant::storage::get_data_path())
                .unwrap();
            let url = format!("/{}?token={token}", path.to_str().unwrap());
            let response = client.get(url.clone()).dispatch().await;
            let public = matches!(hash.as_str(), PUBLIC | PUBLIC_VIDEO);
            if !public {
                assert_eq!(response.status(), Status::NotFound, "{url}");
            } else if path.starts_with("object/imported") {
                assert_eq!(response.status(), Status::Forbidden, "{url}");
            } else {
                assert_eq!(response.status(), Status::Ok, "{url}");
                assert_eq!(
                    response.headers().get_one("Cache-Control"),
                    Some("no-store")
                );
            }
            let response = owner(client.get(url.clone()), true).dispatch().await;
            assert_eq!(response.status(), Status::Ok, "owner: {url}");
            assert_eq!(
                response.headers().get_one("Cache-Control"),
                Some(if path.starts_with("object/imported") {
                    "private, no-store"
                } else {
                    "no-store"
                })
            );
            // Share access is an explicit grant, bounded by current album membership.
            let response = client
                .get(url)
                .header(Header::new("x-album-id", ALBUM))
                .header(Header::new("x-share-id", "fixture-share"))
                .dispatch()
                .await;
            let member = matches!(hash.as_str(), PUBLIC | PRIVATE | PRIVATE_VIDEO);
            if !member {
                assert_eq!(response.status(), Status::NotFound);
            } else if !path.starts_with("object/imported") {
                assert_eq!(response.status(), Status::Ok);
            }
        }
    }

    let shared = client
        .post("/get/prefetch")
        .header(ContentType::JSON)
        .header(Header::new("x-album-id", ALBUM))
        .header(Header::new("x-share-id", "fixture-share"))
        .dispatch()
        .await;
    assert_eq!(shared.status(), Status::Ok);
    let shared: PrefetchReturn = shared.into_json().await.unwrap();
    let response = client
        .get(snapshot_path("data", &shared))
        .header(Header::new("x-album-id", ALBUM))
        .header(Header::new("x-share-id", "fixture-share"))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let shared_hashes: HashSet<_> = response
        .into_json::<Vec<Value>>()
        .await
        .unwrap()
        .into_iter()
        .map(|data| {
            let record = &data["abstractData"];
            assert!(data.get("audience").is_none());
            assert!(!record.to_string().contains("private-gps-marker"));
            assert!(record.get("alias").is_none());
            record["id"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        shared_hashes,
        HashSet::from([
            PUBLIC.to_owned(),
            PRIVATE.to_owned(),
            PRIVATE_VIDEO.to_owned()
        ])
    );
    assert_eq!(
        client
            .get(snapshot_path("data", &shared))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );

    let previous = snapshot(client, false, "descending").await;
    assert_eq!(
        owner(
            client
                .put("/put/edit_photo_audience")
                .json(&serde_json::json!({
                    "hash": PUBLIC,
                    "audience": "private",
                })),
            true,
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    for route in ["data", "rows", "scroll"] {
        assert_eq!(
            client
                .get(snapshot_path(route, &previous))
                .dispatch()
                .await
                .status(),
            Status::Unauthorized
        );
        assert_eq!(
            client
                .get(snapshot_path(route, &shared))
                .header(Header::new("x-album-id", ALBUM))
                .header(Header::new("x-share-id", "fixture-share"))
                .dispatch()
                .await
                .status(),
            Status::Unauthorized
        );
    }
    let response = client
        .post("/post/renew-timestamp-token")
        .header(ContentType::JSON)
        .body(serde_json::json!({ "token": previous.token }).to_string())
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Unauthorized);
    assert_eq!(
        client
            .post("/post/renew-timestamp-token")
            .json(&serde_json::json!({"token": shared.token}))
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share"))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    let current = snapshot(client, false, "descending").await;
    aggregates::checks(client, true).await;
    assert_eq!(
        hashes(client, &current, false).await,
        HashSet::from([PUBLIC_VIDEO.to_owned(), ALBUM.to_owned()])
    );
    assert_eq!(
        client
            .get(format!("/get/resource/{PUBLIC}"))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );
    let data = records
        .iter()
        .find(|data| data.hash().as_str() == PUBLIC)
        .unwrap();
    let token = ClaimsHash::new(id(PUBLIC), 0, true).encode();
    let relative = data.small_thumbnail_path_for_version(7);
    let relative = relative
        .strip_prefix(crate::public::constant::storage::get_data_path())
        .unwrap();
    assert_eq!(
        client
            .get(format!("/{}?token={token}", relative.display()))
            .dispatch()
            .await
            .status(),
        Status::NotFound
    );

    // Old tokens lack a viewer/revision and must not regain access after upgrade.
    let old = serde_json::json!({"resolvedShareOpt": null, "timestamp": current.prefetch.timestamp, "exp": Claims::new_admin().exp});
    let old = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &old,
        &jsonwebtoken::EncodingKey::from_secret(KEY),
    )
    .unwrap();
    assert_eq!(
        client
            .get(format!(
                "/get/get-data?timestamp={}&start=0&end=100&token={old}",
                current.prefetch.timestamp
            ))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    aggregates::share_revocation_checks(client, records).await;
}

async fn private_checks(client: &Client, records: &[AbstractData]) {
    aggregates::private_gallery_checks(client).await;
    let owner_snapshot = snapshot(client, true, "descending").await;
    assert_eq!(
        hashes(client, &owner_snapshot, true).await.len(),
        records.len() - 1
    );
    assert_eq!(
        client
            .post("/get/prefetch")
            .header(ContentType::JSON)
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .get(format!("/get/resource/{PUBLIC}"))
            .dispatch()
            .await
            .status(),
        Status::Unauthorized
    );
    for route in ["data", "rows", "scroll"] {
        assert_eq!(
            client
                .get(snapshot_path(route, &owner_snapshot))
                .dispatch()
                .await
                .status(),
            Status::Unauthorized
        );
    }
    for data in records
        .iter()
        .filter(|data| !matches!(data, AbstractData::Album(_)))
    {
        let token = ClaimsHash::new(data.hash(), 0, true).encode();
        for path in [
            data.imported_path(),
            data.compressed_path(),
            data.thumbnail_path_for_version(7),
            data.small_thumbnail_path_for_version(7),
        ] {
            let path = path
                .strip_prefix(crate::public::constant::storage::get_data_path())
                .unwrap();
            assert_eq!(
                client
                    .get(format!("/{}?token={token}", path.display()))
                    .dispatch()
                    .await
                    .status(),
                Status::Unauthorized
            );
        }
    }
}

#[rocket::async_test]
async fn http_child() {
    let Some(root) = std::env::var_os("UROCISSA_AUDIENCE_TEST_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("fixture-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(KEY.to_vec()).unwrap());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    let records = seed();
    let client = Client::untracked(
        rocket::custom(rocket::Config {
            port: 0,
            log_level: rocket::config::LogLevel::Off,
            ..rocket::Config::debug_default()
        })
        .attach(cache_control_fairing())
        .mount(
            "/",
            routes![
                prefetch,
                get_data,
                get_rows,
                get_scroll_bar,
                get_resource,
                get_search_facets,
                get_albums,
                compressed_file,
                small_thumbnail_file,
                imported_file,
                renew_timestamp_token,
                renew_hash_token,
                edit_share,
                edit_photo_audience,
                crate::router::put::edit_auto_tags::edit_auto_tags,
                crate::router::put::auto_tag_decision::decide,
                crate::router::put::auto_tag_decision::preview,
                crate::router::put::edit_tag::edit_tag,
                crate::router::embeddings::get_auto_tags,
                upload,
            ],
        ),
    )
    .await
    .unwrap();
    if std::env::var_os("UROCISSA_DOWNLOAD_TEST").is_some() {
        media::download_revocation(&client, &records).await;
        return;
    }
    search::checks(&client).await;
    auto_tags::checks(&client).await;
    if std::env::var_os("UROCISSA_FEEDBACK_TEST").is_some() {
        tag_feedback::checks(&client).await;
        return;
    }
    if crate::router::fairing::guard_gallery::public_gallery_enabled() {
        public_checks(&client, &records).await;
    } else {
        private_checks(&client, &records).await;
    }
    mutations::check(&client).await;
}

fn isolated(public: bool, download: bool) {
    isolated_mode(public, download, false);
}

fn isolated_mode(public: bool, download: bool, feedback: bool) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.env_clear();
    if download {
        command.env("UROCISSA_DOWNLOAD_TEST", "1");
    }
    if feedback {
        command.env("UROCISSA_FEEDBACK_TEST", "1");
    }
    let mut child = command
        .env("UROCISSA_AUDIENCE_TEST_ROOT", root.path())
        .env("UROCISSA_PUBLIC_GALLERY", if public { "1" } else { "0" })
        .args([
            "--exact",
            "router::audience_http_tests::http_child",
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
            panic!("Audience HTTP fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    if feedback {
        for line in String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter(|line| line.starts_with("500-photo"))
        {
            eprintln!("public={public}: {line}");
        }
    }
    assert!(
        output.status.success(),
        "public={public}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn public_audience_http() {
    isolated(true, false);
}

#[test]
fn authored_feedback_http() {
    isolated_mode(true, false, true);
    isolated_mode(false, false, true);
}

#[test]
fn disabled_public_gallery_fails_closed_http() {
    isolated(false, false);
}
