//! Enumerate the production router, including future mounts, rather than maintaining
//! a list of mutations. Globals and any accidental writes stay in a disposable child.
use std::process::Command;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use rocket::http::{Header, Method, Status};
use rocket::local::asynchronous::Client;
use serde_json::{Value, json};

use crate::public::constant::storage::DATA_PATH;
use crate::public::structure::config::{APP_CONFIG, AppConfig};

fn run_mode(public: bool) {
    let root = tempfile::tempdir().unwrap();
    let backend = root.path().join("gallery-backend");
    std::fs::create_dir_all(&backend).unwrap();
    std::fs::create_dir_all(root.path().join("gallery-frontend/dist/assets")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_ROUTE_AUTH_TEST_ROOT", root.path())
        .env("UROCISSA_PUBLIC_GALLERY", if public { "1" } else { "0" })
        .env("ROCKET_LOG_LEVEL", "off")
        .current_dir(backend)
        .args([
            "--exact",
            "router::anonymous_route_tests::route_auth_child",
            "--nocapture",
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "anonymous route gate: public={public}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("anonymous route gate timed out: public={public}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn public_gallery_rejects_anonymous_non_get_routes() {
    run_mode(true);
}

#[test]
fn private_gallery_rejects_anonymous_non_get_routes() {
    run_mode(false);
}

fn probe_body(path: &str) -> Value {
    if path == "/get/prefetch" {
        return json!({"Trashed": false});
    }
    if path == "/post/authenticate" {
        // Login accepts an owner password by design. An anonymous bad password
        // must fail, and must not be exempted from the route enumeration.
        return json!("incorrect-dummy-password");
    }
    if path == "/put/album_audience" {
        return json!({
            "albumId": "dummy-album", "audience": "public", "publishPhotos": false
        });
    }
    // Valid shapes for the mounted JSON handlers let their authorization checks
    // run. A future route with a different schema must add a valid probe here:
    // 400/404/415/422/500 do not count as authorization rejection.
    json!({
        "timestamp": 1, "index": 0, "indexArray": [0], "deleteList": [0],
        "albumId": "dummy-album", "coverHash": "dummy-photo", "hash": "dummy-photo",
        "title": "dummy-title", "description": "dummy-description", "audience": "private",
        "addAlbumsArray": [], "removeAlbumsArray": [],
        "addTagsArray": [], "removeTagsArray": [],
        "shareId": "dummy-share", "share": {
            "url": "dummy-share", "description": "dummy-description",
            "showMetadata": false, "showDownload": false, "showUpload": false, "exp": 0
        },
        "showMetadata": false, "showDownload": false, "showUpload": false, "exp": 0,
        "name": "dummy-search", "context": "home", "query": "tag:dummy", "ids": [],
        "path": "/dummy-import", "operations": ["exif"],
        "paused": true,
        "token": "invalid-token", "expiredHashToken": "invalid-token"
    })
}

async fn anonymous_failures(client: &Client, public: bool) -> Vec<String> {
    let routes = client
        .rocket()
        .routes()
        .filter(|route| route.method != Method::Get)
        .collect::<Vec<_>>();
    assert!(!routes.is_empty(), "the gate must exercise mounted routes");
    println!(
        "Checking {} mounted non-GET routes, public={public}",
        routes.len()
    );
    let mut failures = Vec::new();
    for route in routes {
        let path = route
            .uri
            .path()
            .trim_start_matches('/')
            .split('/')
            .map(|segment| {
                if segment.starts_with('<') {
                    "anonymous-probe"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        let path = format!("/{path}");
        let request = client.req(route.method, &path);
        let request = if matches!(
            path.as_str(),
            "/upload" | "/put/regenerate-thumbnail-with-frame"
        ) {
            let file = if path == "/upload" { "file" } else { "frame" };
            request
                .header(Header::new("Content-Type", "multipart/form-data; boundary=auth-probe"))
                .body(format!(
                    "--auth-probe\r\nContent-Disposition: form-data; name=\"{file}\"; filename=\"dummy.jpg\"\r\nContent-Type: image/jpeg\r\n\r\ndummy-image\r\n\
                     --auth-probe\r\nContent-Disposition: form-data; name=\"hash\"\r\n\r\ndummy-photo\r\n\
                     --auth-probe\r\nContent-Disposition: form-data; name=\"lastModified\"\r\n\r\n0\r\n\
                     --auth-probe--\r\n"
                ))
        } else {
            request.json(&probe_body(&path))
        };
        let response = request.dispatch().await;
        // Prefetch is a public READ implemented as POST for its query body.
        // Pin this exact existing route; never exempt a prefix or all POSTs.
        let public_read = public
            && route.method == Method::Post
            && route.uri.path() == "/get/prefetch"
            && route.name.as_deref() == Some("prefetch");
        let rejected = matches!(response.status().code, 401 | 403);
        if if public_read {
            response.status() != Status::Ok
        } else {
            !rejected
        } {
            failures.push(format!(
                "{} {} ({:?}) returned {} instead of {}",
                route.method,
                route.uri,
                route.name,
                response.status(),
                if public_read {
                    "200 (public read)"
                } else {
                    "401/403"
                }
            ));
        }
    }
    failures
}

#[rocket::async_test]
async fn route_auth_child() {
    let Some(root) = std::env::var_os("UROCISSA_ROUTE_AUTH_TEST_ROOT") else {
        return;
    };
    DATA_PATH
        .set(std::path::PathBuf::from(root).join("data"))
        .unwrap();
    let mut config = AppConfig::default();
    config.private.password = Some("dummy-owner-password".into());
    config.private.auth_key = Some("dummy-route-auth-signing-key".into());
    // Writes are enabled so read-only-mode rejection cannot mask missing auth.
    config.public.read_only_mode = false;
    APP_CONFIG.set(RwLock::new(config.clone())).unwrap();
    let client = Client::untracked(super::builder::build_rocket_with_config(config))
        .await
        .unwrap();
    let failures = anonymous_failures(
        &client,
        super::fairing::guard_gallery::public_gallery_enabled(),
    )
    .await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[post("/__unguarded_write")]
fn unguarded_write() -> Status {
    Status::NoContent
}

#[rocket::async_test]
async fn enumeration_detects_a_new_unguarded_route() {
    let client = Client::untracked(rocket::build().mount("/", routes![unguarded_write]))
        .await
        .unwrap();
    let failures = anonymous_failures(&client, true).await;
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("/__unguarded_write"));
    assert!(failures[0].contains("204 No Content"));
}
