use super::*;
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use rocket::{
    http::{ContentType, Status},
    local::blocking::Client,
};

fn sample() -> ReviewRound {
    serde_json::from_value(serde_json::json!({
        "id":"round", "kind":"random", "capturedAt":1, "model":"fixture", "vocabulary":"fixture",
        "materializedAt":1, "libraryRevision":1, "population":100, "plant":"", "separationConfirmed":false,
        "afterCapturedAt":null, "feedback":"", "effortSeconds":0,
        "photos":[{"hash":"fixture", "version":0, "timestamp":1, "role":"random", "enabled":true,
            "pending":false,"before":[{"concept":"plant","label":"plant","judgment":null}],
            "after":null,"seenBefore":false,"seenAfter":false}]
    })).unwrap()
}

#[test]
fn incomplete_stale_and_after_decisions_are_refused() {
    let mut round = sample();
    let mut decision = Decision {
        kind: "random".into(),
        round: "round".into(),
        hash: "fixture".into(),
        after: false,
        judgments: vec![None],
        feedback: String::new(),
        effort_seconds: 5,
    };
    assert!(apply(&mut round, &decision).is_err());
    assert!(!round.photos[0].seen_before);
    decision.judgments = vec![Some(Judgment::Uncertain)];
    apply(&mut round, &decision).unwrap();
    assert_eq!(
        round.photos[0].before[0].judgment,
        Some(Judgment::Uncertain)
    );
    assert!(round.photos[0].seen_before);
    decision.round = "stale".into();
    assert!(apply(&mut round, &decision).is_err());
    decision.round = "round".into();
    decision.after = true;
    assert!(apply(&mut round, &decision).is_err());
}

#[test]
fn acceptance_http_guards() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "router::acceptance::tests::guard_child"])
        .env("UROCISSA_ACCEPTANCE_TEST", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn guard_child() {
    if std::env::var_os("UROCISSA_ACCEPTANCE_TEST").is_none() {
        return;
    }
    let mut config = AppConfig::default();
    config.private.password = Some("acceptance-fixture-password".into());
    config.private.auth_key = Some("acceptance-fixture-signing-key".into());
    APP_CONFIG.set(std::sync::RwLock::new(config)).unwrap();
    let client =
        Client::tracked(rocket::build().mount("/", routes![get, start, decide, capture_after]))
            .unwrap();
    assert_eq!(
        client.get("/get/acceptance-review").dispatch().status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .post("/post/acceptance-review")
            .header(ContentType::JSON)
            .body(r#"{"kind":"random"}"#)
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .put("/put/acceptance-review")
            .header(ContentType::JSON)
            .body(r#"{"kind":"random","round":"x","hash":"x","after":false,"judgments":[]}"#)
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .put("/put/acceptance-review/after/x")
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    let share = crate::router::claims::Claims::new_share(
        crate::public::structure::album::ResolvedShare::default(),
    )
    .encode();
    for path in [
        "/get/acceptance-review",
        "/get/acceptance-review?kind=garden",
    ] {
        assert_eq!(
            client
                .get(path)
                .cookie(rocket::http::Cookie::new("jwt", share.clone()))
                .dispatch()
                .status(),
            Status::Unauthorized
        );
    }
    assert_eq!(
        client
            .post("/post/acceptance-review")
            .header(ContentType::JSON)
            .body("{}")
            .dispatch()
            .status(),
        Status::Unauthorized
    );
    assert_eq!(
        client
            .put("/put/acceptance-review")
            .header(ContentType::JSON)
            .body("{}")
            .dispatch()
            .status(),
        Status::Unauthorized
    );
}
