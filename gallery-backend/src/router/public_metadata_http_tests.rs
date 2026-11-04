//! Real routes and guards with disposable records, files and signing credentials.
use std::process::Command;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use arrayvec::ArrayString;
use rocket::http::{ContentType, Cookie, Header, Status};
use rocket::local::asynchronous::{Client, LocalRequest};
use serde_json::{Value, json};

use crate::operations::public_metadata::{ViewerKind, presentation_timestamp};
use crate::public::constant::storage::DATA_PATH;
use crate::public::db::tree::TREE;
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::album::{Album, Share};
use crate::public::structure::common::FileModify;
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use crate::public::structure::image::{ImageCombined, ImageMetadata};
use crate::public::structure::object::{ObjectSchema, ObjectType};
use crate::public::structure::video::{VideoCombined, VideoMetadata};
use crate::router::claims::{Claims, claims_hash::ClaimsHash};
use crate::router::get::get_data::{get_data, get_rows, get_scroll_bar};
use crate::router::get::get_img::imported_file;
use crate::router::get::get_prefetch::prefetch;
use crate::router::get::get_resource::get_resource;

const IMAGE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const VIDEO: &str = "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ALBUM: &str = "sntwjj2vnqq66wrxc5w5h1rn7t14k29d5wbjuj6abxhiun88881mtem1c06ez74u";
const CAPTURE_TIME: i64 = 1_781_454_912_345;
const VIDEO_CAPTURE_TIME: i64 = 1_778_990_112_345;
const DUMMY_KEY: &[u8] = b"public-metadata-fixture-signing-key";

fn id(value: &str) -> ArrayString<64> {
    ArrayString::from(value).unwrap()
}

#[test]
fn public_and_private_metadata_http() {
    for public in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env("UROCISSA_METADATA_TEST_ROOT", root.path())
            .env("UROCISSA_PUBLIC_GALLERY", if public { "1" } else { "0" })
            .args([
                "--exact",
                "router::public_metadata_http_tests::http_child",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "public={public}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("metadata HTTP check timed out: public={public}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn seed() {
    let mut image = ImageMetadata::new(id(IMAGE), 1000, 1920, 1080, "jpg".into());
    image.albums.insert(id(ALBUM));
    image.exif_vec = [
        ("Make", "Camera"),
        ("FNumber", "f/2.8"),
        ("GPSLatitude", "private-gps-marker"),
        ("Software", "private-software-marker"),
        ("FutureUnknownField", "private-new-marker"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    image.alias.push(FileModify {
        file: "/private/original-filename.jpg".into(),
        modified: CAPTURE_TIME,
        scan_time: CAPTURE_TIME,
    });
    let mut video = VideoMetadata::new(id(VIDEO), 2000, 1280, 720, "mp4".into());
    video.albums = image.albums.clone();
    video.alias = image.alias.clone();
    video.alias[0].modified = VIDEO_CAPTURE_TIME;
    video.alias[0].scan_time = VIDEO_CAPTURE_TIME;
    video.duration = 2.5;
    video.exif_vec = [
        ("codec_name", "h264"),
        ("duration", "2.5"),
        ("TAG:location", "private-location-marker"),
        ("filename", "/private/movie.mp4"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    let mut album = Album::new(id(ALBUM), Some("Public album".into()));
    album.created_time = u128::try_from(CAPTURE_TIME).unwrap();
    album.start_time = Some(u128::try_from(CAPTURE_TIME).unwrap());
    album.end_time = album.start_time;
    album.last_modified_time = album.created_time;
    album.share_list.insert(
        id("test-share"),
        Share {
            show_metadata: true,
            show_download: true,
            ..Share::default()
        },
    );
    let records = vec![
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id(IMAGE), ObjectType::Image),
            metadata: image,
        }),
        AbstractData::Video(VideoCombined {
            object: ObjectSchema::new(id(VIDEO), ObjectType::Video),
            metadata: video,
        }),
        album.into_abstract_data(),
    ];
    TREE.store
        .write(|table| -> anyhow::Result<()> {
            for record in &records {
                table.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    *TREE.state.write().unwrap() = crate::public::db::tree::state::TreeState::from_records(records);
}

fn as_viewer<'a>(request: LocalRequest<'a>, principal: &str) -> LocalRequest<'a> {
    match principal {
        "owner" => request.cookie(Cookie::new(
            "jwt",
            Claims::new_admin().encode_with_key(DUMMY_KEY),
        )),
        "invalid" => request.cookie(Cookie::new("jwt", "invalid-owner-token")),
        "expired" => {
            let mut claims = Claims::new_admin();
            claims.exp = 1;
            request.cookie(Cookie::new("jwt", claims.encode_with_key(DUMMY_KEY)))
        }
        "share" => request
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "test-share")),
        _ => request,
    }
}

fn check_record(response: &Value, owner: bool) {
    let record = &response["abstractData"];
    let capture_time = if record["type"] == "video" {
        VIDEO_CAPTURE_TIME
    } else {
        CAPTURE_TIME
    };
    if owner {
        assert!(record.get("updateAt").is_some());
        assert_eq!(response["timestamp"], capture_time);
        if record["type"] == "image" {
            assert_eq!(
                record["exifVec"]["FutureUnknownField"],
                "private-new-marker"
            );
            assert_eq!(record["alias"][0]["file"], "/private/original-filename.jpg");
        }
    } else {
        let serialized = record.to_string();
        assert!(!serialized.contains("private-"), "{serialized}");
        for field in ["alias", "phash", "updateAt", "createdTime", "shareList"] {
            assert!(record.get(field).is_none(), "{field}: {record}");
        }
        assert_eq!(
            response["timestamp"],
            presentation_timestamp(ViewerKind::Anonymous, capture_time)
        );
        match record["type"].as_str().unwrap() {
            "image" => assert_eq!(
                record["exifVec"],
                json!({"Make": "Camera", "FNumber": "f/2.8"})
            ),
            "video" => assert_eq!(
                record["exifVec"],
                json!({"codec_name": "h264", "duration": "2.5"})
            ),
            "album" => {
                assert!(record["startTime"].is_null());
                assert!(record["endTime"].is_null());
            }
            kind => panic!("unexpected record: {kind}"),
        }
    }
}

#[rocket::async_test]
#[allow(clippy::too_many_lines)]
async fn http_child() {
    let Some(root) = std::env::var_os("UROCISSA_METADATA_TEST_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("dummy-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(DUMMY_KEY.to_vec()).unwrap());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    seed();
    let original = DATA_PATH
        .get()
        .unwrap()
        .join(format!("object/imported/01/{IMAGE}.jpg"));
    std::fs::create_dir_all(original.parent().unwrap()).unwrap();
    std::fs::write(original, b"original fixture").unwrap();
    let client = Client::untracked(
        rocket::custom(rocket::Config {
            port: 0,
            log_level: rocket::config::LogLevel::Off,
            ..rocket::Config::debug_default()
        })
        .attach(super::fairing::cache_control_fairing::cache_control_fairing())
        .mount(
            "/",
            routes![
                imported_file,
                get_data,
                get_rows,
                get_scroll_bar,
                prefetch,
                get_resource
            ],
        ),
    )
    .await
    .unwrap();

    // An old allowOriginal=true capability must not grant originals to a guest.
    let capability = ClaimsHash::new(id(IMAGE), 123, true).encode();
    for principal in ["guest", "invalid", "expired", "owner", "share"] {
        let response = as_viewer(
            client.get(format!("/object/imported/01/{IMAGE}.jpg")),
            principal,
        )
        .header(Header::new("Authorization", format!("Bearer {capability}")))
        .dispatch()
        .await;
        if matches!(principal, "owner" | "share") {
            assert_eq!(response.status(), Status::Ok, "original: {principal}");
            assert_eq!(
                response.headers().get_one("Cache-Control"),
                Some("private, no-store")
            );
            assert_eq!(response.into_bytes().await.unwrap(), b"original fixture");
        } else {
            assert!(
                matches!(response.status().code, 401 | 403),
                "original: {principal}"
            );
        }
    }
    let response = as_viewer(
        client.get(format!("/object/imported/01/{IMAGE}.jpg")),
        "owner",
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Unauthorized);

    let owner_prefetch = as_viewer(
        client.post("/get/prefetch").header(ContentType::JSON),
        "owner",
    )
    .dispatch()
    .await
    .into_json::<Value>()
    .await
    .unwrap();
    for principal in ["owner", "guest", "invalid", "expired", "share"] {
        let owner = principal == "owner";
        let public = super::fairing::guard_gallery::public_gallery_enabled();
        if !public && matches!(principal, "guest" | "invalid" | "expired") {
            continue;
        }
        let response = as_viewer(
            client.post("/get/prefetch").header(ContentType::JSON),
            principal,
        )
        .dispatch()
        .await;
        assert_eq!(response.status(), Status::Ok, "prefetch: {principal}");
        let prefetched = response.into_json::<Value>().await.unwrap();
        let snapshot = prefetched["prefetch"]["timestamp"].as_i64().unwrap();
        let token = prefetched["token"].as_str().unwrap();
        let length = prefetched["prefetch"]["dataLength"].as_u64().unwrap();
        assert_eq!(length, if principal == "share" { 2 } else { 3 });
        let response = as_viewer(
            client.get(format!(
                "/get/get-data?timestamp={snapshot}&start=0&end={length}"
            )),
            principal,
        )
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await;
        assert_eq!(response.status(), Status::Ok);
        let data = response.into_json::<Vec<Value>>().await.unwrap();
        for record in &data {
            check_record(record, owner);
            let token = record["token"].as_str().unwrap();
            if token.is_empty() {
                assert_eq!(record["abstractData"]["type"], "album");
            } else {
                let capability = super::fairing::auth_utils::my_decode_token::<ClaimsHash>(
                    token,
                    &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
                )
                .unwrap();
                assert_eq!(capability.allow_original, owner || principal == "share");
            }
        }
        assert_eq!(data.last().unwrap()["abstractData"]["id"], VIDEO);
        // Presentation redaction must never reorder the snapshot.
        let repeated = as_viewer(
            client.get(format!(
                "/get/get-data?timestamp={snapshot}&start=0&end={length}"
            )),
            principal,
        )
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .await
        .into_json::<Vec<Value>>()
        .await
        .unwrap();
        assert_eq!(
            data.iter()
                .map(|r| &r["abstractData"]["id"])
                .collect::<Vec<_>>(),
            repeated
                .iter()
                .map(|r| &r["abstractData"]["id"])
                .collect::<Vec<_>>()
        );
        let rows = client
            .get(format!("/get/get-rows?timestamp={snapshot}&index=0"))
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .dispatch()
            .await
            .into_json::<Value>()
            .await
            .unwrap();
        assert_eq!(
            rows["displayElements"].as_array().unwrap().len(),
            usize::try_from(length).unwrap()
        );
        assert!(
            rows["displayElements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|element| element
                    .as_object()
                    .unwrap()
                    .keys()
                    .all(|key| matches!(key.as_str(), "displayWidth" | "displayHeight")))
        );
        let scrollbar = client
            .get(format!("/get/get-scroll-bar?timestamp={snapshot}"))
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .dispatch()
            .await
            .into_json::<Vec<Value>>()
            .await
            .unwrap();
        assert_eq!(
            scrollbar
                .iter()
                .map(|boundary| boundary["month"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![6, 5]
        );
        for boundary in scrollbar {
            assert_eq!(boundary.as_object().unwrap().len(), 3);
            assert_eq!(boundary["year"], 2026);
            assert!(boundary["index"].as_u64().unwrap() < length);
        }
        for hash in [IMAGE, VIDEO, ALBUM] {
            if principal == "share" {
                continue;
            } // Direct resources use GuardGallery.
            let response = as_viewer(client.get(format!("/get/resource/{hash}")), principal)
                .dispatch()
                .await;
            assert_eq!(response.status(), Status::Ok);
            check_record(&response.into_json::<Value>().await.unwrap()["data"], owner);
        }
    }
    // Stealing an owner's timestamp capability cannot elevate metadata access.
    if super::fairing::guard_gallery::public_gallery_enabled() {
        let snapshot = owner_prefetch["prefetch"]["timestamp"].as_i64().unwrap();
        let token = owner_prefetch["token"].as_str().unwrap();
        let data = client
            .get(format!("/get/get-data?timestamp={snapshot}&start=0&end=3"))
            .header(Header::new("Authorization", format!("Bearer {token}")))
            .dispatch()
            .await
            .into_json::<Vec<Value>>()
            .await
            .unwrap();
        for record in data {
            check_record(&record, false);
        }
    }
}
