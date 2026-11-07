use std::{
    process::Command,
    sync::RwLock,
    time::{Duration, Instant},
};

use rocket::{
    http::{Cookie, Header, Status},
    local::blocking::Client,
};

use crate::public::structure::{
    album::{ResolvedShare, Share},
    config::{APP_CONFIG, AppConfig},
};
use crate::router::claims::claims::Claims;

#[test]
fn embedding_status_and_pause_are_owner_only() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_EMBEDDING_HTTP_TEST", "1")
        .env("ROCKET_LOG_LEVEL", "off")
        .args([
            "--exact",
            "router::embeddings::tests::http_child",
            "--nocapture",
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("HTTP gate timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn http_child() {
    if std::env::var_os("UROCISSA_EMBEDDING_HTTP_TEST").is_none() {
        return;
    }
    let mut config = AppConfig::default();
    config.private.password = Some("fixture-owner-password".into());
    config.private.auth_key = Some("fixture-only-signing-key".into());
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    let client = Client::tracked(rocket::build().mount(
        "/",
        routes![super::get_embedding_status, super::pause_embedding_backfill,],
    ))
    .unwrap();

    for token in [
        None,
        Some(
            Claims {
                exp: Claims::new_admin().exp,
                ..Claims::new_public()
            }
            .encode(),
        ),
        Some(
            Claims::new_share(ResolvedShare::new(
                arrayvec::ArrayString::from("fixture-album").unwrap(),
                None,
                Share::default(),
            ))
            .encode(),
        ),
    ] {
        let mut get = client.get("/get/embedding-status");
        let mut put = client
            .put("/put/embedding-backfill")
            .header(Header::new("Content-Type", "application/json"))
            .body(r#"{"paused":true}"#);
        if let Some(token) = token {
            get = get.cookie(Cookie::new("jwt", token.clone()));
            put = put.cookie(Cookie::new("jwt", token));
        }
        assert_eq!(get.dispatch().status(), Status::Unauthorized);
        assert_eq!(put.dispatch().status(), Status::Unauthorized);
    }
    // Share headers confer no owner access, including on the status endpoint.
    assert_eq!(
        client
            .get("/get/embedding-status")
            .header(Header::new("x-album-id", "album"))
            .header(Header::new("x-share-id", "share"))
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    let owner = Claims::new_admin().encode();
    assert_eq!(
        client
            .get("/get/embedding-status")
            .cookie(Cookie::new("jwt", owner.clone()))
            .dispatch()
            .status(),
        Status::NotFound
    );
    crate::operations::embedding::install_test_service();
    let response = client
        .get("/get/embedding-status")
        .cookie(Cookie::new("jwt", owner.clone()))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let status: serde_json::Value = response.into_json().unwrap();
    assert_eq!(status["pendingPhotos"], 0);
    let response = client
        .put("/put/embedding-backfill")
        .cookie(Cookie::new("jwt", owner.clone()))
        .header(Header::new("Content-Type", "application/json"))
        .body(r#"{"paused":true}"#)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_json::<serde_json::Value>().unwrap()["backfillPaused"],
        true
    );
    APP_CONFIG
        .get()
        .unwrap()
        .write()
        .unwrap()
        .public
        .read_only_mode = true;
    assert_eq!(
        client
            .put("/put/embedding-backfill")
            .cookie(Cookie::new("jwt", owner))
            .header(Header::new("Content-Type", "application/json"))
            .body(r#"{"paused":false}"#)
            .dispatch()
            .status(),
        Status::MethodNotAllowed
    );
}
