//! Latency and memory benchmark. Requires an exported model:
//! `TAGGER_MODEL_DIR=... TAGGER_CPU_THREADS=2 cargo test --release --test benchmark -- --ignored --nocapture`

mod common;

use std::time::{Duration, Instant};

use common::{fixtures, load};
use gallery_tagger::{embedder::Embedder, preprocess::decode_image};

const SAMPLES: usize = 20;

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn peak_rss_mib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| {
            value
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map_or(0, |kib| kib / 1024)
}

#[test]
#[ignore = "requires an exported model: set TAGGER_MODEL_DIR"]
fn latency_and_memory() {
    let started = Instant::now();
    let embedder = load();
    let load_time = started.elapsed();
    let bytes = std::fs::read(fixtures().join("images/stork_pair.jpg")).unwrap();
    let texts = vec!["kid on a bike at dusk".to_owned()];
    embedder
        .embed_image(&decode_image(&bytes).unwrap())
        .unwrap();
    embedder.embed_texts(&texts).unwrap();

    let mut decode_and_image = Vec::new();
    let mut text = Vec::new();
    for _ in 0..SAMPLES {
        let started = Instant::now();
        embedder
            .embed_image(&decode_image(&bytes).unwrap())
            .unwrap();
        decode_and_image.push(started.elapsed());
        let started = Instant::now();
        embedder.embed_texts(&texts).unwrap();
        text.push(started.elapsed());
    }
    let info = embedder.info();
    println!(
        "BENCH model={} device={} threads={} precision={} load={:.1}s image(decode+infer)={:.0}ms text={:.0}ms peak_rss={}MiB",
        info.model_id,
        info.device,
        std::env::var("TAGGER_CPU_THREADS").unwrap_or_else(|_| "auto".into()),
        std::env::var("TAGGER_CPU_PRECISION").unwrap_or_else(|_| "auto".into()),
        load_time.as_secs_f32(),
        median(decode_and_image).as_secs_f64() * 1000.0,
        median(text).as_secs_f64() * 1000.0,
        peak_rss_mib(),
    );
}
