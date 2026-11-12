//! Exercise the production page routes with a disposable HTML shell, as in the image.
use std::process::Command;
use std::time::{Duration, Instant};

use rocket::http::{ContentType, Status};
use rocket::local::asynchronous::Client;

const HTML: &str = "<!doctype html><html><body>review-page-fixture</body></html>";

#[test]
fn review_deep_links_serve_the_frontend() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), HTML).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("UROCISSA_PAGE_ROUTE_TEST", "1")
        .env("ROCKET_LOG_LEVEL", "off")
        .current_dir(root.path())
        .args([
            "--exact",
            "router::page_route_tests::page_route_child",
            "--nocapture",
        ])
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "review page route regression");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("review page route regression timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[rocket::async_test]
async fn page_route_child() {
    if std::env::var_os("UROCISSA_PAGE_ROUTE_TEST").is_none() {
        return;
    }
    let client = Client::untracked(rocket::build().mount("/", super::get::generate_get_routes()))
        .await
        .unwrap();
    for path in [
        "/login", // Known-good control: the HTML fixture must actually be served.
        "/review",
        "/review?concept=pierre",
        "/review?concept=personal%3Apierre",
        "/review/view/photo?concept=pierre",
        "/review/view/album/read?concept=pierre",
        "/review/view/album/read/view/photo?concept=pierre",
    ] {
        let response = client.get(path).dispatch().await;
        assert_eq!(response.status(), Status::Ok, "{path}");
        assert_eq!(response.content_type(), Some(ContentType::HTML), "{path}");
        assert_eq!(
            response.into_string().await.as_deref(),
            Some(HTML),
            "{path}"
        );
    }
    for path in [
        "/not-a-gallery-page",
        "/get/missing-api",
        "/review/not-a-page",
    ] {
        assert_eq!(
            client.get(path).dispatch().await.status(),
            Status::NotFound,
            "{path}"
        );
    }
}
