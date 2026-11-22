use super::*;
use serde_json::json;
use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
};

pub(super) async fn checks(client: &Client) {
    let path = "/post/concept_candidates";
    let body = json!({"hash": PUBLIC});
    for request in [
        client.post(path).json(&body),
        client
            .post(path)
            .json(&body)
            .header(Header::new("x-album-id", ALBUM))
            .header(Header::new("x-share-id", "fixture-share")),
    ] {
        assert_eq!(request.dispatch().await.status(), Status::Unauthorized);
    }
    for hash in [PUBLIC_VIDEO, ALBUM, "invalid"] {
        assert_eq!(
            owner(client.post(path).json(&json!({"hash": hash})), true)
                .dispatch()
                .await
                .status(),
            Status::ServiceUnavailable
        );
    }
    TREE.store.set_auto_tags_enabled(&[PUBLIC], false).unwrap();
    assert_eq!(
        owner(client.post(path).json(&body), true)
            .dispatch()
            .await
            .status(),
        Status::ServiceUnavailable
    );
    TREE.store.set_auto_tags_enabled(&[PUBLIC], true).unwrap();
    let socket = std::env::var_os("UROCISSA_CONCEPT_SOCKET").unwrap();
    let listener = UnixListener::bind(socket).unwrap();
    let worker = std::thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut header = [0; 4];
            stream.read_exact(&mut header).unwrap();
            let mut image = vec![0; u32::from_be_bytes(header) as usize];
            stream.read_exact(&mut image).unwrap();
            assert_eq!(image, b"fixture-media");
            let reply = br#"{"candidates":["bicycle","rhubarb"]}"#;
            stream
                .write_all(&u32::try_from(reply.len()).unwrap().to_be_bytes())
                .unwrap();
            stream.write_all(reply).unwrap();
        }
    });
    let response = owner(client.post(path).json(&body), true).dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_json::<Value>().await.unwrap(),
        json!({"candidates":["bicycle","rhubarb"], "decisions": {}})
    );
    assert!(
        TREE.store
            .auto_tag_rejections_for(PUBLIC)
            .unwrap()
            .is_empty()
    );
    assert!(
        !TREE
            .store
            .reader()
            .unwrap()
            .get(PUBLIC)
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains("rhubarb")
    );
    for (label, action) in [("Bike", "confirm"), ("rhubarb", "reject")] {
        assert_eq!(
            owner(
                client
                    .put("/put/concept_candidate_decision")
                    .json(&json!({"hash": PUBLIC, "label": label, "action": action})),
                true
            )
            .dispatch()
            .await
            .status(),
            Status::Ok
        );
    }
    let response = owner(client.post(path).json(&body), true).dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_json::<Value>().await.unwrap(),
        json!({"candidates":["bicycle","rhubarb"], "decisions":{"bicycle":"confirmed","rhubarb":"rejected"}})
    );
    assert_eq!(
        owner(
            client
                .put("/put/concept_candidate_decision")
                .json(&json!({"hash": PUBLIC, "label": "rhubarb", "action": "clearRejection"})),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    let response = owner(client.post(path).json(&body), true).dispatch().await;
    assert_eq!(
        response.into_json::<Value>().await.unwrap(),
        json!({"candidates":["bicycle","rhubarb"], "decisions":{"bicycle":"confirmed"}})
    );
    worker.join().unwrap();
}

pub(super) async fn real_worker_checks(client: &Client, records: &[AbstractData]) {
    let photo = records
        .iter()
        .find(|record| record.hash().as_str() == PUBLIC)
        .unwrap();
    std::fs::write(
        photo.thumbnail_path_for_version(7),
        include_bytes!("../../../../gallery-tagger/tests/fixtures/images/cat.jpg"),
    )
    .unwrap();
    let response = owner(
        client
            .post("/post/concept_candidates")
            .json(&json!({"hash": PUBLIC})),
        true,
    )
    .dispatch()
    .await;
    assert_eq!(response.status(), Status::Ok);
    let response = response.into_json::<Value>().await.unwrap();
    let label = response["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|label| label.contains("cat"))
        .unwrap();
    let decision = json!({"hash": PUBLIC, "label": label, "action": "confirm"});
    let rejection = json!({"hash": PUBLIC, "label": "water", "action": "reject"});
    assert_eq!(
        owner(
            client
                .put("/put/concept_candidate_decision")
                .json(&rejection),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert_eq!(
        owner(
            client
                .put("/put/concept_candidate_decision")
                .json(&decision),
            true
        )
        .dispatch()
        .await
        .status(),
        Status::Ok
    );
    assert!(
        TREE.store
            .reader()
            .unwrap()
            .get(PUBLIC)
            .unwrap()
            .unwrap()
            .into_value()
            .tag()
            .contains(label)
    );
}
