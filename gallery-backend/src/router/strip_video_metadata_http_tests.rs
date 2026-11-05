//! Actual HTTP guards and background execution with disposable media only.
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use rocket::http::{Cookie, Header, Method, Status};
use rocket::local::asynchronous::{Client, LocalRequest};
use serde_json::Value;

use crate::public::constant::storage::DATA_PATH;
use crate::public::structure::album::{ResolvedShare, Share};
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use crate::router::claims::Claims;

const KEY: &[u8] = b"strip-video-metadata-fixture-key-only";
const START: &str = "/post/video/strip-metadata";
const STATUS: &str = "/get/video/strip-metadata";

fn run_mode(read_only: bool) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("object/compressed")).unwrap();
    std::fs::create_dir_all(root.path().join("object/imported")).unwrap();
    std::fs::write(
        root.path().join("object/imported/original.mp4"),
        b"original fixture sentinel",
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_STRIP_HTTP_ROOT", root.path())
        .env(
            "UROCISSA_STRIP_HTTP_READ_ONLY",
            if read_only { "1" } else { "0" },
        )
        .env("UROCISSA_PUBLIC_GALLERY", "1")
        .current_dir(root.path())
        .args([
            "--exact",
            "router::strip_video_metadata_http_tests::http_child",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "strip HTTP gate: read_only={read_only}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("strip HTTP gate exceeded 45 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read(root.path().join("object/imported/original.mp4")).unwrap(),
        b"original fixture sentinel"
    );
}

#[test]
fn only_owner_can_start_and_observe_fixture_job() {
    run_mode(false);
}

#[test]
fn read_only_refuses_owner_job() {
    run_mode(true);
}

fn request<'a>(
    client: &'a Client,
    method: Method,
    path: &'a str,
    principal: &str,
) -> LocalRequest<'a> {
    let request = client.req(method, path);
    let mut claims = match principal {
        "owner" | "expired" => Claims::new_admin(),
        "public" => {
            let mut public = Claims::new_public();
            public.exp = Claims::new_admin().exp;
            public
        }
        "share" => Claims::new_share(ResolvedShare::new(
            arrayvec::ArrayString::from("fixture-album").unwrap(),
            None,
            Share::default(),
        )),
        "share-headers" => {
            return request
                .header(Header::new("x-album-id", "fixture-album"))
                .header(Header::new("x-share-id", "fixture-share"));
        }
        "invalid" => return request.cookie(Cookie::new("jwt", "invalid-jwt")),
        "anonymous" => return request,
        _ => panic!("unexpected fixture principal"),
    };
    if principal == "expired" {
        claims.exp = 1;
    }
    request.cookie(Cookie::new("jwt", claims.encode_with_key(KEY)))
}

async fn completed_status(client: &Client, job_id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let response = request(client, Method::Get, STATUS, "owner")
                .dispatch()
                .await;
            assert_eq!(response.status(), Status::Ok);
            let status = response.into_json::<Value>().await.unwrap();
            assert_eq!(status["jobId"], job_id);
            if status["state"] != "running" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("empty fixture job exceeded 10 seconds")
}

#[rocket::async_test]
async fn http_child() {
    let Some(root) = std::env::var_os("UROCISSA_STRIP_HTTP_ROOT") else {
        return;
    };
    DATA_PATH.set(root.into()).unwrap();
    let read_only = std::env::var("UROCISSA_STRIP_HTTP_READ_ONLY").unwrap() == "1";
    let mut config = AppConfig::default();
    config.private.password = Some("dummy-owner-password".into());
    config.private.auth_key = Some(String::from_utf8(KEY.to_vec()).unwrap());
    config.public.read_only_mode = read_only;
    APP_CONFIG.set(RwLock::new(config)).unwrap();
    let client = Client::untracked(
        rocket::custom(rocket::Config::figment().merge(("log_level", "off"))).mount(
            "/",
            routes![
                super::strip_video_metadata::start_video_metadata_strip,
                super::strip_video_metadata::get_video_metadata_strip,
            ],
        ),
    )
    .await
    .unwrap();
    for principal in [
        "anonymous",
        "invalid",
        "expired",
        "public",
        "share",
        "share-headers",
    ] {
        for (method, path) in [(Method::Post, START), (Method::Get, STATUS)] {
            assert_eq!(
                request(&client, method, path, principal)
                    .dispatch()
                    .await
                    .status(),
                Status::Unauthorized,
                "{principal} must not access {path}"
            );
        }
    }
    let status = request(&client, Method::Get, STATUS, "owner")
        .dispatch()
        .await;
    assert_eq!(status.status(), Status::Ok);
    assert_eq!(status.into_json::<Value>().await.unwrap(), Value::Null);
    for _ in 0..2 {
        let response = request(&client, Method::Post, START, "owner")
            .dispatch()
            .await;
        if read_only {
            assert_eq!(response.status(), Status::MethodNotAllowed);
            assert!(crate::tasks::actor::strip_video_metadata::strip_job_status().is_none());
            return;
        }
        assert_eq!(response.status(), Status::Accepted);
        let accepted = response.into_json::<Value>().await.unwrap();
        let job_id = accepted["jobId"].as_str().unwrap();
        let status = completed_status(&client, job_id).await;
        assert_eq!(status["state"], "completed");
        assert_eq!(status["error"], Value::Null);
        for counter in ["total", "changed", "unchanged", "failed"] {
            assert_eq!(status["report"][counter], 0);
        }
        assert_eq!(status["report"]["verification"]["passed"], true);
    }
}
