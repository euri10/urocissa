use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use super::{Embedder, HttpEmbedder, ImageEmbedding, ModelInfo};

fn server(body: String, status: u16, delay: Duration) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut socket = loop {
            if let Ok((socket, _)) = listener.accept() {
                break socket;
            }
            assert!(Instant::now() < deadline, "client did not connect");
            thread::sleep(Duration::from_millis(5));
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            socket.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let headers = String::from_utf8(request.clone()).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut payload = vec![0; length];
        socket.read_exact(&mut payload).unwrap();
        request.extend(payload);
        thread::sleep(delay);
        let _ = write!(
            socket,
            "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8(request).unwrap()
    });
    (url, handle)
}

fn model() -> ModelInfo {
    ModelInfo {
        model_id: "test-model".into(),
        embedding_dim: 2,
        device: "CPU".into(),
    }
}

#[test]
fn sidecar_v1_info_and_raw_image_contract() {
    let mut info = serde_json::to_value(model()).unwrap();
    info["logit_scale"] = serde_json::json!(112.6);
    let (url, server) = server(info.to_string(), 200, Duration::ZERO);
    assert_eq!(HttpEmbedder::new(&url).unwrap().info().unwrap(), model());
    assert!(server.join().unwrap().starts_with("GET /v1/info HTTP/1.1"));
    let (url, server) = self::server(
        r#"{"model_id":"test-model","embedding":[0.6,0.8]}"#.into(),
        200,
        Duration::ZERO,
    );
    let output = HttpEmbedder::new(&url)
        .unwrap()
        .embed_image(b"jpeg-bytes".to_vec())
        .unwrap();
    output.validate(&model()).unwrap();
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /v1/embed/image HTTP/1.1"));
    assert!(request.contains("content-type: image/jpeg"));
    assert!(request.ends_with("jpeg-bytes"));
}

#[test]
fn rejects_broken_sidecar_vectors_and_model_identity() {
    for (id, vector) in [
        ("other-model", vec![0.6, 0.8]),
        ("test-model", vec![1.0]),
        ("test-model", vec![0.0, 0.0]),
        ("test-model", vec![f32::NAN, 1.0]),
    ] {
        assert!(
            ImageEmbedding {
                model_id: id.into(),
                embedding: vector
            }
            .validate(&model())
            .is_err()
        );
    }
    for info in [
        ModelInfo {
            model_id: String::new(),
            ..model()
        },
        ModelInfo {
            embedding_dim: 0,
            ..model()
        },
        ModelInfo {
            embedding_dim: 8193,
            ..model()
        },
    ] {
        assert!(info.validate().is_err());
    }
}

#[test]
fn http_errors_malformed_json_and_slow_tagger_are_bounded() {
    for (body, status) in [("{}", 503), ("not-json", 200), ("{}", 200)] {
        let (url, server) = server(body.into(), status, Duration::ZERO);
        assert!(HttpEmbedder::new(&url).unwrap().info().is_err());
        server.join().unwrap();
    }
    let (url, server) = server(
        serde_json::to_string(&model()).unwrap(),
        200,
        Duration::from_millis(200),
    );
    let client = HttpEmbedder::with_timeout(&url, Duration::from_millis(50)).unwrap();
    assert!(client.info().is_err());
    server.join().unwrap();
}

#[test]
fn rejects_credential_urls() {
    for url in [
        "file:///tmp/tagger",
        "http://user:password@localhost",
        "http://localhost?key=x",
        "http://localhost/#fragment",
    ] {
        assert!(HttpEmbedder::new(url).is_err(), "{url}");
    }
}
