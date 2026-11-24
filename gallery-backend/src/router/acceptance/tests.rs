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
fn snapshot_gathering_allows_other_writers_and_preserves_their_judgments() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = std::sync::Arc::new(crate::storage::DataStore::initialize_empty(
        &root.path().join("db.redb"),
    )?);
    let mut initial = sample();
    initial.kind = "garden".into();
    initial.photos[0].role = "heldOut".into();
    initial.photos[0].seen_before = true;
    store.update_acceptance("garden", |_| Ok(initial))?;
    let mut writer = None;
    let result = capture_and_save(&store, "round", |mut captured| {
        let sibling = store.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        writer = Some(std::thread::spawn(move || {
            let saved = sibling.update_acceptance("garden", |round| {
                let mut round = round.unwrap();
                round.feedback = "concurrent owner review".into();
                round.photos[0].before[0].judgment = Some(Judgment::Yes);
                Ok(round)
            });
            let _ = sender.send(saved.is_ok());
        }));
        ensure!(
            receiver.recv_timeout(std::time::Duration::from_secs(2)) == Ok(true),
            "Writer blocked while gathering snapshot"
        );
        captured.photos[0].after = Some(Vec::new());
        captured.after_captured_at = Some(2);
        Ok(captured)
    });
    writer.unwrap().join().unwrap();
    let saved = result?;
    assert_eq!(saved.feedback, "concurrent owner review");
    assert_eq!(saved.photos[0].before[0].judgment, Some(Judgment::Yes));
    assert!(saved.photos[0].after.is_some());
    assert!(
        capture_and_save(&store, "round", Ok).is_err(),
        "After snapshot is immutable"
    );
    Ok(())
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
