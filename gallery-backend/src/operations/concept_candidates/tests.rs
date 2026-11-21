#[cfg(unix)]
#[test]
fn framed_worker_responses_are_bounded_and_validated() {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixStream,
    };

    fn run(response: &[u8]) -> anyhow::Result<Vec<String>> {
        let (mut client, mut worker) = UnixStream::pair()?;
        let response = response.to_vec();
        let thread = std::thread::spawn(move || {
            let mut length = [0; 4];
            worker.read_exact(&mut length).unwrap();
            assert_eq!(u32::from_be_bytes(length), 3);
            let mut image = [0; 3];
            worker.read_exact(&mut image).unwrap();
            assert_eq!(&image, b"jpg");
            worker
                .write_all(&u32::try_from(response.len()).unwrap().to_be_bytes())
                .unwrap();
            let _ = worker.write_all(&response);
        });
        let result = super::exchange(&mut client, b"jpg");
        thread.join().unwrap();
        result
    }
    assert_eq!(
        run(br#"{"candidates":["tree","rhubarb"]}"#).unwrap(),
        ["tree", "rhubarb"]
    );
    assert!(run(br#"{"error":"busy"}"#).is_err());
    assert!(run(br#"{"candidates":["bad\nlabel"]}"#).is_err());
    assert!(run(br#"{"candidates":["black person"]}"#).is_err());
    assert!(run(br#"{"candidates":["unreviewed object"]}"#).is_err());
    assert!(run(&vec![b'x'; 8193]).is_err());
    assert!(run(br#"{"candidates":["a","b","c","d","e","f","g","h","i"]}"#).is_err());
}

#[cfg(unix)]
#[test]
#[ignore = "explicit local model integration with public fixture"]
fn local_worker_public_fixture() {
    let image = include_bytes!("../../../../gallery-tagger/tests/fixtures/images/cat.jpg");
    let labels = super::request(image).unwrap();
    assert!(
        labels.iter().any(|label| label.contains("cat")),
        "{labels:?}"
    );
}
