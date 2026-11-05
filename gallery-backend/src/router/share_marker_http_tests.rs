//! Owner-only share summaries use current membership, trash state and photo audience.
use std::{
    process::Command,
    sync::RwLock,
    time::{Duration, Instant},
};

use arrayvec::ArrayString;
use rocket::{
    http::{Cookie, Header, Status},
    local::asynchronous::Client,
};
use serde_json::Value;

use crate::{
    public::{
        constant::storage::DATA_PATH,
        db::tree::{TREE, state::TreeState},
        structure::{
            abstract_data::AbstractData,
            album::{Album, Share},
            audience::Audience,
            config::{APP_CONFIG, AppConfig},
            image::{ImageCombined, ImageMetadata},
            object::{ObjectSchema, ObjectType},
            video::{VideoCombined, VideoMetadata},
        },
    },
    router::{claims::Claims, get::get_list::get_albums},
};

const KEY: &[u8] = b"share-marker-fixture-key-only";

fn id(value: &str) -> ArrayString<64> {
    ArrayString::from(value).unwrap()
}

fn seed() {
    let mut records = Vec::new();
    for name in [
        "public-private",
        "private-public",
        "missing",
        "trash",
        "empty",
        "video",
    ] {
        let mut album = Album::new(id(name), Some(name.into()));
        album.share_list.insert(id("share"), Share::default());
        records.push(album.into_abstract_data());
        if name == "empty" {
            continue;
        }
        let hash = id(&format!("{name}-photo"));
        let mut photo = if name == "video" {
            let mut metadata = VideoMetadata::new(hash, 1, 1, 1, "mp4".into());
            metadata.albums.insert(id(name));
            AbstractData::Video(VideoCombined {
                object: ObjectSchema::new(hash, ObjectType::Video),
                metadata,
            })
        } else {
            let mut metadata = ImageMetadata::new(hash, 1, 1, 1, "jpg".into());
            metadata.albums.insert(id(name));
            AbstractData::Image(ImageCombined {
                object: ObjectSchema::new(hash, ObjectType::Image),
                metadata,
            })
        };
        photo.set_trashed(name == "trash");
        records.push(photo);
    }
    TREE.store
        .write(|table| -> anyhow::Result<()> {
            for record in &records {
                table.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    TREE.store
        .set_album_audience("public-private", Audience::Public, false)
        .unwrap();
    TREE.store
        .set_photo_audience("public-private-photo", Audience::Private)
        .unwrap();
    TREE.store
        .set_photo_audience("private-public-photo", Audience::Public)
        .unwrap();
    *TREE.state.write().unwrap() = TreeState::from_records(records);
}

#[rocket::async_test]
async fn http_child() {
    let Some(root) = std::env::var_os("UROCISSA_SHARE_MARKER_TEST_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("fixture-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(KEY.to_vec()).unwrap());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    seed();
    let client = Client::untracked(
        rocket::custom(rocket::Config {
            port: 0,
            log_level: rocket::config::LogLevel::Off,
            ..rocket::Config::debug_default()
        })
        .mount("/", routes![get_albums]),
    )
    .await
    .unwrap();

    let response = client
        .get("/get/get-albums")
        .cookie(Cookie::new("jwt", Claims::new_admin().encode_with_key(KEY)))
        .dispatch()
        .await;
    assert_eq!(response.status(), Status::Ok);
    let albums = response.into_json::<Vec<Value>>().await.unwrap();
    assert_eq!(albums.len(), 6);
    for album in albums {
        let name = album["albumId"].as_str().unwrap();
        assert_eq!(
            album["hasPrivatePhotos"],
            matches!(name, "public-private" | "missing" | "video"),
            "{name}"
        );
        assert_eq!(album["shareList"].as_object().unwrap().len(), 1);
    }
    for share in [false, true] {
        let mut request = client.get("/get/get-albums");
        if share {
            request = request
                .header(Header::new("x-album-id", "public-private"))
                .header(Header::new("x-share-id", "share"));
        }
        let response = request.dispatch().await;
        assert_eq!(response.status(), Status::Ok);
        let albums = response.into_json::<Vec<Value>>().await.unwrap();
        assert_eq!(albums.len(), 1);
        assert!(albums[0].get("hasPrivatePhotos").is_none());
        assert_eq!(albums[0]["shareList"], serde_json::json!({}));
    }

    // Re-read the current audience, without reloading or rebuilding the album.
    TREE.store
        .set_photo_audience("public-private-photo", Audience::Public)
        .unwrap();
    let response = client
        .get("/get/get-albums")
        .cookie(Cookie::new("jwt", Claims::new_admin().encode_with_key(KEY)))
        .dispatch()
        .await;
    let albums = response.into_json::<Vec<Value>>().await.unwrap();
    assert_eq!(
        albums
            .iter()
            .find(|album| album["albumId"] == "public-private")
            .unwrap()["hasPrivatePhotos"],
        false
    );
}

#[test]
fn owner_share_private_photo_markers_http() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_SHARE_MARKER_TEST_ROOT", root.path())
        .env("UROCISSA_PUBLIC_GALLERY", "1")
        .args([
            "--exact",
            "router::share_marker_http_tests::http_child",
            "--nocapture",
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Share marker HTTP fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
