use super::*;
use crate::operations::embedding::client::ImageEmbedding;
use crate::storage::embeddings::StoredEmbedding;
use rand::{RngExt, SeedableRng, rngs::StdRng};

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    for value in vector {
        *value /= norm;
    }
}

struct CachedOnly;
impl Embedder for CachedOnly {
    fn info(&self) -> Result<ModelInfo> {
        unreachable!()
    }
    fn embed_image(&self, _: Vec<u8>) -> Result<ImageEmbedding> {
        panic!("no image inference during feedback")
    }
    fn embed_text(&self, _: &str) -> Result<ImageEmbedding> {
        panic!("no text inference during feedback")
    }
}

fn resident_kib() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

const PHOTOS: usize = 6500;

fn fixture() -> Result<(tempfile::TempDir, AutoTags, PhotoInputs, Vec<String>)> {
    const DIM: usize = 768;
    let directory = tempfile::tempdir()?;
    let store = EmbeddingStore::open(&directory.path().join("derived.redb"))?;
    let tags = AutoTags::new(store.clone())?;
    let model = ModelInfo {
        model_id: tags.catalog.model_id.clone(),
        embedding_dim: DIM,
        device: "fixture".into(),
        logit_scale: Some(112.0),
        logit_bias: Some(-16.0),
    };
    let mut rng = StdRng::seed_from_u64(23);
    let vectors: Vec<Vec<f32>> = tags
        .catalog
        .concepts
        .iter()
        .map(|_| {
            let mut vector: Vec<f32> = (0..DIM).map(|_| rng.random_range(-1.0..1.0)).collect();
            normalize(&mut vector);
            vector
        })
        .collect();
    store.set_model(&model)?;
    store.put_concept_vectors(
        &model.model_id,
        &tags.catalog.version,
        &ConceptVectors {
            scale: 112.0,
            bias: -16.0,
            vectors: tags
                .catalog
                .concepts
                .iter()
                .zip(&vectors)
                .map(|(concept, vector)| (concept.id.clone(), vector.clone()))
                .collect(),
        },
    )?;
    let mut inputs = HashMap::new();
    let mut untagged = Vec::new();
    for index in 0..PHOTOS {
        let id = blake3::hash(&index.to_le_bytes()).to_hex().to_string();
        if index % 9 != 0 && index % 19 != 0 {
            untagged.push(id.clone());
        }
        let mut vector: Vec<f32> = (0..DIM).map(|_| rng.random_range(-1.0..1.0)).collect();
        normalize(&mut vector);
        for _ in 0..rng.random_range(0..4) {
            let concept = &vectors[rng.random_range(0..vectors.len())];
            let weight = rng.random_range(0.2..1.5);
            for (value, source) in vector.iter_mut().zip(concept) {
                *value += weight * source;
            }
        }
        normalize(&mut vector);
        store.put(
            &id,
            &model.model_id,
            0,
            &StoredEmbedding {
                thumbnail_version: 0,
                vector,
            },
        )?;
        inputs.insert(
            id,
            Input {
                version: 0,
                tags: if index % 9 == 0 {
                    vec!["cat".into()]
                } else {
                    vec![]
                },
                enabled: true,
                rejections: if index % 19 == 0 {
                    vec!["dog".into()]
                } else {
                    vec![]
                },
            },
        );
    }
    tags.prepare(&model, &CachedOnly)?;
    Ok((directory, tags, inputs, untagged))
}

// Run alone in the Linux production runtime: RSS includes the whole process.
// The 128 MiB ceiling leaves >2x headroom over the batched fixture (~62 MiB).
#[test]
#[ignore = "isolated Linux production-runtime memory regression"]
fn repeated_feedback_memory() -> Result<()> {
    let (_directory, tags, mut inputs, untagged) = fixture()?;
    let mut confirmations = PHOTOS.div_ceil(9);
    tags.materialize(&inputs, &[("cat".into(), confirmations)])?;
    let baseline = resident_kib();
    eprintln!("feedback memory: initial {baseline} KiB");
    for (round, id) in untagged.iter().take(30).enumerate() {
        let input = inputs.get_mut(id).unwrap();
        if round % 2 == 0 {
            input.tags.push("cat".into());
            confirmations += 1;
        } else {
            input.rejections.push("cat".into());
        }
        if round % 2 == 0 {
            tags.materialize(&inputs, &[("cat".into(), confirmations)])?;
        } else {
            std::thread::scope(|scope| {
                scope
                    .spawn(|| tags.materialize(&inputs, &[("cat".into(), confirmations)]))
                    .join()
                    .unwrap()
            })?;
        }
        let resident = resident_kib();
        eprintln!("feedback memory: round {round}: {resident} KiB");
        assert!(
            resident < 128 * 1024,
            "feedback exceeded the isolated fixture's memory budget: {resident} KiB"
        );
    }
    Ok(())
}
