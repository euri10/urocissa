//! Real compression and ffprobe checks, isolated from installed media and globals.
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{VideoCompressionOutcome, generate_compressed_video_to};
use crate::public::constant::storage::DATA_PATH;
use crate::public::structure::abstract_data::AbstractData;
use crate::public::structure::object::{ObjectSchema, ObjectType};
use crate::public::structure::video::{VideoCombined, VideoMetadata};

const GLOBAL_TAGS: &[(&str, &str)] = &[
    ("location", "+48.8566+002.3522/"),
    ("com.apple.quicktime.location.ISO6709", "+48.8566+002.3522/"),
    ("creation_time", "2024-01-02T03:04:05.000000Z"),
    ("title", "private source title"),
    ("artist", "private source author"),
    ("copyright", "private source copyright"),
    ("comment", "/private/source/video.mov"),
    ("private_custom", "unknown source metadata"),
];

fn run(command: &mut Command) -> Output {
    let output = command.stdin(Stdio::null()).output().unwrap();
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn probe(path: &Path) -> Value {
    let output = run(Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-show_chapters",
            "-of",
            "json",
        ])
        .arg(path));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn isolated() -> bool {
    // DATA_PATH is process-global; the parent owns a disposable child's directory.
    if std::env::var("UROCISSA_VIDEO_METADATA_TEST").as_deref() != Ok("1") {
        let root = tempfile::tempdir().unwrap();
        let test_name = format!(
            "{}::compressed_videos_strip_source_metadata_and_remain_streamable",
            module_path!().split_once("::").unwrap().1
        );
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env("UROCISSA_VIDEO_METADATA_TEST", "1")
            .current_dir(root.path())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "isolated compression test failed: {status}"
                );
                return true;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated compression test exceeded 90 seconds");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    DATA_PATH.set(std::env::current_dir().unwrap()).unwrap();
    false
}

fn tagged_fixture(width: u32, height: u32) -> AbstractData {
    let hash = arrayvec::ArrayString::from(&format!("{height:064x}")).unwrap();
    let data = AbstractData::Video(VideoCombined {
        object: ObjectSchema::new(hash, ObjectType::Video),
        metadata: VideoMetadata::new(hash, 0, width, height, "mp4".into()),
    });
    let source = data.imported_path();
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let chapters = source.with_extension("ffmetadata");
    fs::write(
        &chapters,
        ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=500\ntitle=private chapter\n",
    )
    .unwrap();
    let mut fixture = Command::new("ffmpeg");
    fixture
        .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={width}x{height}:rate=4"))
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-f",
            "ffmetadata",
            "-i",
        ])
        .arg(&chapters)
        .args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map_chapters",
            "2",
            "-t",
            "0.5",
            "-c:v",
            "mpeg4",
            "-threads:v",
            "1",
            "-c:a",
            "aac",
            "-movflags",
            "+use_metadata_tags",
            "-metadata:s:v:0",
            "handler_name=private video handler",
            "-metadata:s:v:0",
            "language=fra",
            "-metadata:s:a:0",
            "handler_name=private audio handler",
            "-metadata:s:a:0",
            "language=deu",
            "-metadata:s",
            "creation_time=2024-01-02T03:04:05.000000Z",
        ]);
    for (key, value) in GLOBAL_TAGS {
        fixture.arg("-metadata").arg(format!("{key}={value}"));
    }
    run(fixture.arg(&source));

    let tagged = probe(&source);
    for (key, value) in GLOBAL_TAGS {
        assert_eq!(tagged["format"]["tags"][key], *value, "fixture lacks {key}");
    }
    assert_eq!(tagged["chapters"][0]["tags"]["title"], "private chapter");
    let source_streams: Vec<_> = tagged["streams"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|stream| stream["codec_type"] != "data")
        .collect();
    assert_eq!(source_streams.len(), 2);
    for (stream, handler, language) in [
        (&source_streams[0], "private video handler", "fra"),
        (&source_streams[1], "private audio handler", "deu"),
    ] {
        assert_eq!(stream["tags"]["handler_name"], handler);
        assert_eq!(stream["tags"]["language"], language);
        assert_eq!(
            stream["tags"]["creation_time"],
            "2024-01-02T03:04:05.000000Z"
        );
    }
    data
}

#[test]
fn compressed_videos_strip_source_metadata_and_remain_streamable() {
    if isolated() {
        return;
    }
    // Exercise both the 720p cap and preservation of a smaller source's size.
    for (width, height, expected_width, expected_height) in
        [(1000, 750, 960, 720), (96, 64, 96, 64)]
    {
        let data = tagged_fixture(width, height);
        let source = data.imported_path();
        let original_bytes = fs::read(&source).unwrap();
        let compressed = data.compressed_path();
        fs::create_dir_all(compressed.parent().unwrap()).unwrap();
        assert_eq!(
            generate_compressed_video_to(&data, &compressed, false).unwrap(),
            VideoCompressionOutcome::Video
        );
        assert_eq!(
            fs::read(&source).unwrap(),
            original_bytes,
            "original changed"
        );
        let output = probe(&compressed);
        assert!(super::super::video_metadata_policy::tags_are_clean(&output));
        eprintln!(
            "ffprobe {width}x{height}: compressed tags {}",
            output["format"]["tags"]
        );

        // MP4 muxers generate technical tags even with source metadata disabled.
        // Reject every other key, including unknown future source metadata.
        for (key, value) in output["format"]["tags"].as_object().unwrap() {
            match key.as_str() {
                "major_brand" | "minor_version" | "compatible_brands" => {}
                "encoder" => assert!(value.as_str().unwrap().starts_with("Lavf")),
                _ => panic!("source global metadata survived: {key}={value}"),
            }
        }
        let streams = output["streams"].as_array().unwrap();
        assert_eq!(streams.len(), 2, "video and audio must both survive");
        for stream in streams {
            let handler = match stream["codec_type"].as_str().unwrap() {
                "video" => {
                    assert_eq!(stream["codec_name"], "h264");
                    assert_eq!(stream["width"], expected_width);
                    assert_eq!(stream["height"], expected_height);
                    assert_eq!(stream["pix_fmt"], "yuv420p");
                    assert_eq!(stream["nb_frames"], "2");
                    "VideoHandler"
                }
                "audio" => {
                    assert_eq!(stream["codec_name"], "aac");
                    assert_eq!(stream["sample_rate"], "48000");
                    "SoundHandler"
                }
                other => panic!("unexpected stream: {other}"),
            };
            for (key, value) in stream["tags"].as_object().unwrap() {
                match key.as_str() {
                    "language" => assert_eq!(value, "und"),
                    "handler_name" => assert_eq!(value, handler),
                    "vendor_id" => assert_eq!(value, "[0][0][0][0]"),
                    "encoder" => assert!(value.as_str().unwrap().starts_with("Lavc")),
                    _ => panic!("source stream metadata survived: {key}={value}"),
                }
            }
        }
        let duration = output["format"]["duration"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (0.49..0.6).contains(&duration),
            "duration changed: {duration}"
        );
        run(Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-xerror", "-i"])
            .arg(&compressed)
            .args(["-f", "null", "-"]));

        // Read top-level boxes in this small fixture to verify faststart.
        let bytes = fs::read(&compressed).unwrap();
        let mut offset = 0;
        let mut boxes = Vec::new();
        while offset < bytes.len() {
            let size = usize::try_from(u32::from_be_bytes(
                bytes[offset..offset + 4].try_into().unwrap(),
            ))
            .unwrap();
            assert!(size >= 8 && offset + size <= bytes.len());
            boxes.push(&bytes[offset + 4..offset + 8]);
            offset += size;
        }
        assert!(
            boxes.iter().position(|name| *name == b"moov").unwrap()
                < boxes.iter().position(|name| *name == b"mdat").unwrap(),
            "faststart requires moov before mdat"
        );
    }
}
