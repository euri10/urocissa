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

fn fixture(count: usize) -> Result<(tempfile::TempDir, AutoTags, PhotoInputs, Vec<String>)> {
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
    for index in 0..count {
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
    let (_directory, tags, mut inputs, untagged) = fixture(PHOTOS)?;
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

// Opt in on the target machine; synthetic vectors and feedback never use gallery data.
#[test]
#[ignore = "isolated release-profile full-library performance regression"]
fn full_library_benchmark() -> Result<()> {
    use crate::public::{
        constant::storage::DATA_PATH,
        db::tree::state::TreeState,
        structure::{
            abstract_data::AbstractData,
            image::{ImageCombined, ImageMetadata},
            object::{ObjectSchema, ObjectType},
        },
    };
    use arrayvec::ArrayString;
    use std::time::{Duration, Instant};

    let count = std::env::var("UROCISSA_BENCH_PHOTOS")
        .unwrap_or_else(|_| "6410".into())
        .parse::<usize>()?;
    let samples = std::env::var("UROCISSA_BENCH_SAMPLES")
        .unwrap_or_else(|_| "7".into())
        .parse::<usize>()?;
    ensure!(count >= 1_100 && samples > 0, "invalid benchmark size");
    let (_directory, tags, mut inputs, _) = fixture(count)?;
    for input in inputs.values_mut() {
        input.tags.clear();
        input.rejections.clear();
    }
    // Exercise zero-shot, public reference, learned and example-based modes.
    // Large synthetic classes reproduce only observed aggregate example counts.
    let scenarios = [
        ("building", 29, 8, 0_usize),
        ("bridge", 12, 0, 40),
        ("snow", 51, 7, 60),
        ("cat", 273, 113, 128),
        ("dog", 93, 10, 520),
        ("bird", 127, 21, 630),
        ("horse", 68, 9, 800),
        ("lake", 59, 29, 880),
        ("train", 65, 18, 970),
    ];
    for (name, positives, negatives, offset) in scenarios {
        for index in 0..(positives + negatives) {
            let id = blake3::hash(&(index + offset).to_le_bytes())
                .to_hex()
                .to_string();
            let input = inputs.get_mut(&id).unwrap();
            if index < positives {
                input.tags.push(name.into());
            } else {
                input.rejections.push(name.into());
            }
        }
    }
    let counts: Vec<_> = scenarios
        .iter()
        .map(|(name, positives, _, _)| ((*name).into(), *positives))
        .collect();
    // This ignored test must run in its own process: TREE and DATA_PATH are global.
    let main_directory = tempfile::tempdir()?;
    DATA_PATH
        .set(main_directory.path().to_owned())
        .map_err(|_| anyhow::anyhow!("benchmark requires an isolated test process"))?;
    let main = crate::storage::DataStore::initialize_empty(
        &main_directory.path().join("db/index_v6.redb"),
    )?;
    main.write(|writer| {
        for (id, input) in &inputs {
            for concept in &input.rejections {
                writer.reject_auto_tag(id, concept)?;
            }
        }
        Ok::<_, anyhow::Error>(())
    })?;
    drop(main);
    let records = inputs.iter().map(|(id, input)| {
        let id = ArrayString::<64>::from(id.as_str()).unwrap();
        let mut object = ObjectSchema::new(id, ObjectType::Image);
        object.tags.extend(input.tags.iter().cloned());
        AbstractData::Image(ImageCombined {
            object,
            metadata: ImageMetadata::new(id, 1, 1, 1, "jpg".into()),
        })
    });
    *TREE.state.write().unwrap() = TreeState::from_records(records);
    let (actual_inputs, _) = gallery_inputs()?;
    assert_eq!(actual_inputs, inputs);
    eprintln!(
        "fixture photos={count} dim=768 concepts={} reference_heads={} seed=23 samples={samples}",
        tags.catalog.concepts.len(),
        tags.catalog.references.len()
    );
    let mut expected = None;
    let mut times = Vec::new();
    for round in 0..=samples {
        let start = Instant::now();
        assert!(tags.recalculate()?);
        let elapsed = start.elapsed();
        let status = tags.recalculation_status();
        let changes = status.changes;
        assert_eq!(status.full_photos, count);
        assert_eq!(changes.considered_photos, count);
        assert_eq!(changes.scored_photos, count);
        {
            let guard = tags.prepared.lock().unwrap();
            let methods = &guard.as_ref().unwrap().learning_methods;
            for name in ["building", "horse", "lake"] {
                assert_eq!(methods[name], "similarity");
            }
            for name in ["snow", "cat", "dog", "bird", "train"] {
                assert_eq!(methods[name], "learned");
            }
        }
        let mut rows = tags.store.all_auto_tags()?;
        rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(rows.len(), count);
        let fingerprint = blake3::hash(&serde_json::to_vec(&rows)?);
        if let Some(expected) = expected {
            assert_eq!(fingerprint, expected, "predictions changed between passes");
            assert_eq!((changes.added, changes.removed), (0, 0));
        } else {
            eprintln!("synthetic_fingerprint={fingerprint}");
            expected = Some(fingerprint);
        }
        eprintln!(
            "{}={:.3} ms",
            if round == 0 { "warmup" } else { "sample" },
            elapsed.as_secs_f64() * 1000.0
        );
        if round > 0 {
            times.push(elapsed);
        }
    }
    times.sort_unstable();
    let median = times[times.len() / 2];
    eprintln!(
        "median={:.3} ms min={:.3} ms max={:.3} ms",
        median.as_secs_f64() * 1000.0,
        times[0].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0
    );
    if let Ok(limit) = std::env::var("UROCISSA_BENCH_MAX_MS") {
        assert!(
            median < Duration::from_millis(limit.parse()?),
            "benchmark median {median:?}"
        );
    }
    if std::env::var_os("UROCISSA_BENCH_PROFILE").is_some() {
        let start = Instant::now();
        let (snapshot, _) = gallery_inputs()?;
        eprintln!(
            "profile_input_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(snapshot, inputs);
        profile_materialize(&tags, &inputs, &counts)?;
    }
    Ok(())
}

// Attribution mirrors the bounded materialization loop; normal samples above time the
// actual materialize method, not this diagnostic reconstruction.
fn profile_materialize(
    tags: &AutoTags,
    inputs: &PhotoInputs,
    counts: &[(String, usize)],
) -> Result<()> {
    use std::time::{Duration, Instant};

    let mut guard = tags.prepared.lock().unwrap();
    let prepared = guard.as_mut().unwrap();
    prepared.learning_dirty = true;
    prepared.seen.clear();
    let whole = Instant::now();
    let start = Instant::now();
    tags.refresh_learning(prepared, inputs)?;
    let aliases = AliasIndex::build(
        prepared.engine.vocabulary(),
        counts.iter().map(|(tag, count)| (tag.as_str(), *count)),
    );
    prepared.engine.set_aliases(&aliases);
    prepared.aliases = aliases;
    let learning = start.elapsed();
    let mut reads = Duration::ZERO;
    let mut scoring = Duration::ZERO;
    let mut comparison = Duration::ZERO;
    let mut rows = Vec::new();
    let mut changes = recalculation::Changes::default();
    let pending: Vec<_> = inputs.iter().collect();
    for chunk in pending.chunks(256) {
        let start = Instant::now();
        let mut embedded = Vec::with_capacity(chunk.len());
        for &(id, input) in chunk {
            if let Some(embedding) = tags
                .store
                .get(id, &prepared.model, 0)?
                .filter(|e| e.thumbnail_version == input.version)
            {
                embedded.push((id, input, embedding));
            }
        }
        reads += start.elapsed();
        let start = Instant::now();
        let predictions = tag_batch(prepared, &embedded);
        scoring += start.elapsed();
        let start = Instant::now();
        for ((id, input, _), result) in embedded.iter().zip(predictions) {
            let predictions = result?;
            changes.compare(tags.store.auto_tags(id)?.as_ref(), &predictions);
            rows.push((
                (*id).clone(),
                MaterializedTags {
                    enabled: input.enabled,
                    model_id: prepared.model.clone(),
                    vocabulary_version: tags.catalog.version.clone(),
                    thumbnail_version: input.version,
                    tags: predictions,
                },
            ));
        }
        comparison += start.elapsed();
    }
    assert_eq!(rows.len(), inputs.len());
    assert_eq!((changes.added, changes.removed), (0, 0));
    let start = Instant::now();
    tags.store.put_auto_tags(&rows)?;
    let commit = start.elapsed();
    eprintln!(
        "profile_ms learning={:.3} embedding_reads={:.3} scoring={:.3} previous_and_rows={:.3} commit={:.3} total={:.3}",
        learning.as_secs_f64() * 1000.0,
        reads.as_secs_f64() * 1000.0,
        scoring.as_secs_f64() * 1000.0,
        comparison.as_secs_f64() * 1000.0,
        commit.as_secs_f64() * 1000.0,
        whole.elapsed().as_secs_f64() * 1000.0,
    );
    Ok(())
}
