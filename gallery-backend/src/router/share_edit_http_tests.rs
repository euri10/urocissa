//! Exercise production routes/guards through Rocket's HTTP dispatcher. Each mode
//! runs in a fresh process: configuration, storage and environment are globals.
use std::process::Command;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use arrayvec::ArrayString;
use rocket::http::{Cookie, Header, Method, Status};
use rocket::local::asynchronous::{Client, LocalRequest};
use serde_json::{Value, json};

use crate::public::constant::storage::DATA_PATH;
use crate::public::db::tree::TREE;
use crate::public::db::tree::state::{TargetSet, TreeState};
use crate::public::db::tree_snapshot::{PendingTreeSnapshot, TREE_SNAPSHOT};
use crate::public::db::write_behind::WRITE_BEHIND;
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::album::{Album, Share};
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use crate::public::structure::image::{ImageCombined, ImageMetadata};
use crate::public::structure::object::{ObjectSchema, ObjectType};
use crate::public::structure::saved_search::{SavedSearch, SavedSearchContext};
use crate::router::claims::Claims;
use crate::router::get::get_config::{export_config_handler, get_config_handler};
use crate::router::post::post_upload::upload;
use crate::router::put::edit_album::set_album_title;
use crate::router::put::edit_description::set_user_defined_description;
use crate::router::saved_searches::get_saved_searches;

const SNAPSHOT: i64 = 123;
const DUMMY_KEY: &[u8] = b"infra-xtt-dummy-signing-key-for-tests-only";

fn run_mode(public: bool, read_only: bool) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("INFRA_XTT_TEST_ROOT", root.path())
        .env("INFRA_XTT_READ_ONLY", if read_only { "1" } else { "0" })
        .args([
            "--exact",
            "router::share_edit_http_tests::http_child",
            "--nocapture",
        ]);
    if public {
        command.env("UROCISSA_PUBLIC_GALLERY", "1");
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "public={public}, read_only={read_only}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("HTTP regression timed out: public={public}, read_only={read_only}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn private_share_edits_http() {
    run_mode(false, false);
}

#[test]
fn public_owner_only_http() {
    run_mode(true, false);
}

#[test]
fn private_read_only_http() {
    run_mode(false, true);
}

#[test]
fn public_read_only_http() {
    run_mode(true, true);
}

fn id(value: &str) -> ArrayString<64> {
    ArrayString::from(value).unwrap()
}

fn seed() {
    let mut shared = Album::new(id("shared"), Some("original".into()));
    for (name, password, exp, show_upload) in [
        ("valid", None, 0, true),
        ("locked", Some("dummy-share-password".to_owned()), 0, true),
        ("expired", None, 1, true),
        ("no-upload", None, 0, false),
    ] {
        shared.share_list.insert(
            id(name),
            Share {
                password,
                show_upload,
                exp,
                ..Share::default()
            },
        );
    }
    let mut metadata = ImageMetadata::new(id("member"), 1, 1, 1, "jpg".into());
    metadata.albums.insert(id("shared"));
    let records = vec![
        shared.into_abstract_data(),
        Album::new(id("unrelated"), Some("original".into())).into_abstract_data(),
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id("member"), ObjectType::Image),
            metadata,
        }),
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id("outside"), ObjectType::Image),
            metadata: ImageMetadata::new(id("outside"), 1, 1, 1, "jpg".into()),
        }),
    ];
    TREE.store
        .write(|table| -> anyhow::Result<()> {
            for record in &records {
                table.insert(record)?;
            }
            Ok(())
        })
        .unwrap();
    let state = TreeState::from_records(records);
    // Deliberately an owner/global snapshot: a timestamp must not confer scope.
    let slots = ["shared", "unrelated", "member", "outside"].map(|name| state.find(name).unwrap());
    TREE_SNAPSHOT.in_memory.insert(
        SNAPSHOT,
        PendingTreeSnapshot {
            structural_epoch: state.structural_epoch(),
            identity_epoch: state.identity_epoch(),
            selection_epoch: state.selection_epoch(),
            universe: state.arena.capacity(),
            ordinals: slots.iter().map(|slot| slot.index()).collect(),
            targets: TargetSet::from_slot_refs(slots, state.arena.capacity()),
            scrollbar: Vec::new(),
        },
    );
    *TREE.state.write().unwrap() = state;
}

fn request<'a>(
    client: &'a Client,
    method: Method,
    path: &str,
    principal: &str,
) -> LocalRequest<'a> {
    let path = if principal == "query-share" {
        format!(
            "{path}{}albumId=shared&shareId=valid",
            if path.contains('?') { '&' } else { '?' }
        )
    } else {
        path.to_owned()
    };
    let request = client.req(method, path);
    match principal {
        "owner" => request.cookie(Cookie::new(
            "jwt",
            Claims::new_admin().encode_with_key(DUMMY_KEY),
        )),
        "expired-jwt" => {
            let mut claims = Claims::new_admin();
            claims.exp = 1;
            request.cookie(Cookie::new("jwt", claims.encode_with_key(DUMMY_KEY)))
        }
        "invalid-jwt" => request.cookie(Cookie::new("jwt", "not-a-jwt")),
        "non-owner-jwt" => {
            let mut claims = Claims::new_public();
            claims.exp = Claims::new_admin().exp;
            request.cookie(Cookie::new("jwt", claims.encode_with_key(DUMMY_KEY)))
        }
        "guest" | "query-share" => request,
        "partial-share" => request.header(Header::new("x-album-id", "shared")),
        name => {
            let share = match name {
                "share" => "valid",
                "password-share" | "bad-password" => "locked",
                "expired-share" => "expired",
                "no-upload" => "no-upload",
                _ => "missing",
            };
            let request = request
                .header(Header::new("x-album-id", "shared"))
                .header(Header::new("x-share-id", share));
            if name == "password-share" {
                request.header(Header::new("x-share-password", "dummy-share-password"))
            } else {
                request
            }
        }
    }
}

fn logical_record(name: &str) -> AbstractData {
    let durable = TREE
        .store
        .read(|table| table.get(name))
        .unwrap()
        .map(crate::storage::store::RecordValue::into_value);
    WRITE_BEHIND.logical_record(name, durable).unwrap()
}

#[rocket::async_test]
#[allow(clippy::too_many_lines)]
async fn http_child() {
    let Some(root) = std::env::var_os("INFRA_XTT_TEST_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let public = super::fairing::guard_gallery::public_gallery_enabled();
    let read_only = std::env::var("INFRA_XTT_READ_ONLY").unwrap() == "1";
    let mut config = AppConfig::default();
    config.private.password = Some("dummy-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(DUMMY_KEY.to_vec()).unwrap());
    config.private.saved_searches.push(SavedSearch::new(
        "dummy-private-search".into(),
        SavedSearchContext::Home,
        "private-tag".into(),
    ));
    config
        .public
        .sync_paths
        .insert("/dummy/private/sync".into());
    config.public.read_only_mode = read_only;
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    seed();
    // No listener or external calls: dispatch the actual HTTP routes and guards.
    let client = Client::untracked(
        rocket::custom(rocket::Config {
            port: 0,
            log_level: rocket::config::LogLevel::Off,
            ..rocket::Config::debug_default()
        })
        .mount(
            "/",
            routes![
                set_album_title,
                set_user_defined_description,
                upload,
                get_config_handler,
                export_config_handler,
                get_saved_searches
            ],
        ),
    )
    .await
    .unwrap();

    for principal in [
        "owner",
        "share",
        "query-share",
        "password-share",
        "no-upload",
        "guest",
        "invalid-share",
        "expired-share",
        "bad-password",
        "partial-share",
        "expired-jwt",
        "invalid-jwt",
        "non-owner-jwt",
    ] {
        let owner = principal == "owner";
        let share = matches!(
            principal,
            "share" | "query-share" | "password-share" | "no-upload"
        );
        let editable = owner || (!public && share);
        let expected_write = if read_only {
            Status::MethodNotAllowed
        } else {
            Status::Ok
        };
        for (path, body) in [
            (
                "/put/set_album_title",
                json!({"albumId":"shared", "title":principal}),
            ),
            (
                "/put/set_user_defined_description",
                json!({"index":2, "timestamp":SNAPSHOT, "description":principal}),
            ),
        ] {
            let before = serde_json::to_value(logical_record(if path.ends_with("title") {
                "shared"
            } else {
                "member"
            }))
            .unwrap();
            let response = request(&client, Method::Put, path, principal)
                .json(&body)
                .dispatch()
                .await;
            if editable {
                assert_eq!(response.status(), expected_write, "{principal}: {path}");
            } else {
                assert!(
                    matches!(response.status().code, 400 | 401 | 403 | 404),
                    "{principal}: {path}: {}",
                    response.status()
                );
            }
            let after = serde_json::to_value(logical_record(if path.ends_with("title") {
                "shared"
            } else {
                "member"
            }))
            .unwrap();
            if editable && !read_only {
                assert_eq!(
                    after[if path.ends_with("title") {
                        "title"
                    } else {
                        "description"
                    }],
                    principal
                );
            } else {
                assert_eq!(before, after, "denied request mutated data");
            }
        }
        // An empty multipart body reaches the production upload handler without indexing files.
        let response = request(
            &client,
            Method::Post,
            "/upload?presigned_album_id_opt=shared",
            principal,
        )
        .header(Header::new(
            "Content-Type",
            "multipart/form-data; boundary=xtt",
        ))
        .body("--xtt--\r\n")
        .dispatch()
        .await;
        let upload_allowed = owner || (!public && matches!(principal, "share" | "password-share"));
        assert_eq!(
            response.status(),
            if upload_allowed {
                expected_write
            } else {
                Status::Unauthorized
            },
            "{principal}: upload"
        );

        let response = request(&client, Method::Get, "/get/config", principal)
            .dispatch()
            .await;
        let readable = owner
            || share
            || (public
                && matches!(
                    principal,
                    "guest" | "invalid-jwt" | "expired-jwt" | "non-owner-jwt"
                ));
        if readable {
            assert_eq!(response.status(), Status::Ok, "{principal}: config");
            let value: Value = response.into_json().await.unwrap();
            assert_eq!(value["isAdmin"], owner);
            assert_eq!(
                value["readOnlyMode"],
                read_only || !editable,
                "{principal}: effective write policy"
            );
            for field in ["private", "password", "authKey", "savedSearches"] {
                assert!(value.get(field).is_none(), "leaked {field}");
            }
            if !owner {
                assert_eq!(value["syncPaths"], json!([]));
            }
        } else {
            assert!(!response.status().class().is_success());
        }
        for path in ["/get/config/export", "/get/saved_searches"] {
            let response = request(&client, Method::Get, path, principal)
                .dispatch()
                .await;
            assert_eq!(
                response.status(),
                if owner {
                    Status::Ok
                } else {
                    Status::Unauthorized
                },
                "{principal}: {path}"
            );
        }
    }

    // Neither a guessed album ID nor another user's snapshot grants membership.
    if !public && !read_only {
        for suffix in [
            "presigned_album_id_opt=unrelated",
            "presigned_album_id_opt=shared&presigned_album_ids_opt=%5B%22unrelated%22%5D",
            "presigned_album_id_opt=shared&presigned_tags_opt=%5B%22tag%22%5D",
        ] {
            let response = request(&client, Method::Post, &format!("/upload?{suffix}"), "share")
                .header(Header::new(
                    "Content-Type",
                    "multipart/form-data; boundary=xtt",
                ))
                .body("--xtt--\r\n")
                .dispatch()
                .await;
            assert_eq!(response.status(), Status::Unauthorized);
        }
        for body in [
            json!({"index":1,"timestamp":SNAPSHOT,"description":"forbidden"}),
            json!({"index":3,"timestamp":SNAPSHOT,"description":"forbidden"}),
            json!({"index":0,"timestamp":SNAPSHOT,"description":"forbidden", "selection":{"mode":"allExcept","excludedIndices":[0,1,2]}}),
        ] {
            let response = request(
                &client,
                Method::Put,
                "/put/set_user_defined_description",
                "share",
            )
            .json(&body)
            .dispatch()
            .await;
            assert_eq!(response.status(), Status::Forbidden);
        }
        let response = request(&client, Method::Put, "/put/set_album_title", "share")
            .json(&json!({"albumId":"unrelated", "title":"forbidden"}))
            .dispatch()
            .await;
        assert_eq!(response.status(), Status::Forbidden);
        assert_eq!(
            serde_json::to_value(logical_record("unrelated")).unwrap()["title"],
            "original"
        );
        assert!(serde_json::to_value(logical_record("outside")).unwrap()["description"].is_null());

        // All-except and explicit descriptors are checked after resolution too.
        for selection in [
            json!({"mode":"explicit","indices":[2]}),
            json!({"mode":"allExcept","excludedIndices":[0,1,3]}),
        ] {
            let response = request(&client, Method::Put, "/put/set_user_defined_description", "share")
                .json(&json!({"index":3,"timestamp":SNAPSHOT,"description":"allowed", "selection":selection}))
                .dispatch().await;
            assert_eq!(response.status(), Status::Ok);
            assert_eq!(
                serde_json::to_value(logical_record("member")).unwrap()["description"],
                "allowed"
            );
        }
        // A snapshot made while an object was shared cannot keep granting access
        // after its current album membership is removed.
        TREE.state
            .write()
            .unwrap()
            .query
            .albums
            .remove(&id("shared"));
        let response = request(
            &client,
            Method::Put,
            "/put/set_user_defined_description",
            "share",
        )
        .json(&json!({"index":2,"timestamp":SNAPSHOT,"description":"revoked"}))
        .dispatch()
        .await;
        assert_eq!(response.status(), Status::Forbidden);
        assert_eq!(
            serde_json::to_value(logical_record("member")).unwrap()["description"],
            "allowed"
        );
    }
}
