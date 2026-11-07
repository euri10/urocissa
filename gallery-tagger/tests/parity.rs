//! Parity with the Hugging Face FP32 reference. Requires an exported model:
//! `TAGGER_MODEL_DIR=... cargo test --release -- --include-ignored`.
//! Set `TAGGER_DEVICES=GPU` to check the iGPU path.

mod common;

use common::{cosine, fixtures, load, model_dir, reference};
use gallery_tagger::{
    embedder::Embedder,
    manifest::Manifest,
    preprocess::{TextEncoder, decode_image},
};

const MIN_COSINE: f32 = 0.99;

#[test]
#[ignore = "requires an exported model: set TAGGER_MODEL_DIR"]
fn tokenization_matches_reference_exactly() {
    let manifest = Manifest::load(&model_dir()).unwrap();
    let reference = reference(&manifest.variant);
    let encoder = TextEncoder::from_file(&model_dir().join("tokenizer.json"), &manifest).unwrap();
    for text in &reference.texts {
        assert_eq!(
            encoder.encode(&text.text).unwrap(),
            text.input_ids,
            "tokens for {:?}",
            text.text
        );
    }
}

#[test]
#[ignore = "requires an exported model: set TAGGER_MODEL_DIR"]
fn embeddings_match_reference() {
    let embedder = load();
    let reference = reference(&embedder.info().variant);
    assert!((embedder.info().logit_scale - reference.logit_scale).abs() < 1e-3);
    assert!((embedder.info().logit_bias - reference.logit_bias).abs() < 1e-3);
    let mut worst = (1.0_f32, String::new());
    for (name, expected) in &reference.images {
        let bytes = std::fs::read(fixtures().join("images").join(name)).unwrap();
        let actual = embedder
            .embed_image(&decode_image(&bytes).unwrap())
            .unwrap();
        let similarity = cosine(&actual, expected);
        println!("image {name}: cosine {similarity:.5}");
        if similarity < worst.0 {
            worst = (similarity, name.clone());
        }
    }
    let texts: Vec<String> = reference.texts.iter().map(|t| t.text.clone()).collect();
    let actual = embedder.embed_texts(&texts).unwrap();
    for (vector, expected) in actual.iter().zip(&reference.texts) {
        let similarity = cosine(vector, &expected.embedding);
        println!(
            "text {:?}: cosine {similarity:.5}",
            expected.text.chars().take(32).collect::<String>()
        );
        if similarity < worst.0 {
            worst = (similarity, expected.text.clone());
        }
    }
    println!(
        "device {}: worst cosine {:.5} ({})",
        embedder.info().device,
        worst.0,
        worst.1
    );
    assert!(
        worst.0 >= MIN_COSINE,
        "worst cosine {:.5} for {}",
        worst.0,
        worst.1
    );
}

#[test]
#[ignore = "requires an exported model: set TAGGER_MODEL_DIR"]
fn photos_rank_their_own_caption_first() {
    let embedder = load();
    let captions = [
        "a photo of a stork",
        "a photo of a cat",
        "people riding bicycles",
        "sunset over the ocean",
    ];
    let photos = [
        ("stork_pair.jpg", 0),
        ("stork_fledglings.jpg", 0),
        ("cat.jpg", 1),
        ("bicycles.jpg", 2),
        ("sunset.jpg", 3),
    ];
    let texts: Vec<String> = captions.iter().map(|c| (*c).to_owned()).collect();
    let caption_vectors = embedder.embed_texts(&texts).unwrap();
    for (photo, expected) in photos {
        let bytes = std::fs::read(fixtures().join("images").join(photo)).unwrap();
        let image = embedder
            .embed_image(&decode_image(&bytes).unwrap())
            .unwrap();
        let scores: Vec<f32> = caption_vectors.iter().map(|c| cosine(c, &image)).collect();
        let best = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        assert_eq!(
            best, expected,
            "{photo} ranked {:?} first ({scores:?})",
            captions[best]
        );
    }
}

#[test]
#[ignore = "requires an exported model: set TAGGER_MODEL_DIR"]
fn outputs_are_deterministic_across_restarts() {
    let bytes = std::fs::read(fixtures().join("images/cat.jpg")).unwrap();
    let image = decode_image(&bytes).unwrap();
    let texts = vec!["un chat".to_owned()];
    let first = load();
    let (a_image, a_text) = (
        first.embed_image(&image).unwrap(),
        first.embed_texts(&texts).unwrap(),
    );
    assert_eq!(a_image, first.embed_image(&image).unwrap());
    drop(first);
    let second = load();
    assert_eq!(a_image, second.embed_image(&image).unwrap());
    assert_eq!(a_text, second.embed_texts(&texts).unwrap());
}
