//! Property tests over seeded random libraries, plus the ignored recompute benchmark.
use std::collections::{HashMap, HashSet};

use rand::{RngExt, SeedableRng, rngs::StdRng};

use super::*;

fn unit_vector(rng: &mut StdRng, dim: usize) -> Vec<f32> {
    loop {
        let vector: Vec<f32> = (0..dim).map(|_| rng.random_range(-1.0..1.0)).collect();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.1 {
            return vector.into_iter().map(|value| value / norm).collect();
        }
    }
}

fn normalized(vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    vector.into_iter().map(|value| value / norm).collect()
}

fn naive_dot(left: &[f32], right: &[f32]) -> f32 {
    #[expect(clippy::cast_possible_truncation, reason = "reference precision")]
    let dot = left
        .iter()
        .zip(right)
        .map(|(&l, &r)| f64::from(l) * f64::from(r))
        .sum::<f64>() as f32;
    dot
}

/// A deterministic classifier: some Concepts inactive, some decided by a per-Concept linear
/// probe mixed with the zero-shot probability.
struct RandomClassifier {
    modes: HashMap<String, ConceptMode>,
    probes: HashMap<String, (Vec<f32>, f32)>,
}

impl RandomClassifier {
    fn decision(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        let (probe, threshold) = &self.probes[&concept.id];
        let logit = 4.0 * naive_dot(probe, embedding) + 2.0 * zero_shot - 1.0;
        Classification {
            probability: 1.0 / (1.0 + (-logit).exp()),
            threshold: *threshold,
        }
    }
}

impl ConceptClassifier for RandomClassifier {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        self.modes[&concept.id]
    }

    fn classify(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        self.decision(concept, embedding, zero_shot)
    }
}

struct PhotoData {
    embedding: Vec<f32>,
    user_tags: Vec<String>,
    rejections: Vec<String>,
    auto_tags_off: bool,
}

struct Scenario {
    dim: usize,
    model: ScoreModel,
    concepts: Vec<Concept>,
    photos: Vec<PhotoData>,
    modes: HashMap<String, ConceptMode>,
    probes: HashMap<String, (Vec<f32>, f32)>,
}

impl Scenario {
    #[expect(
        clippy::too_many_lines,
        reason = "one generator keeps the knobs together"
    )]
    fn generate(rng: &mut StdRng, dim: usize, concept_count: usize, photo_count: usize) -> Self {
        let score_model =
            ScoreModel::new(rng.random_range(5.0..30.0), rng.random_range(-8.0..0.0)).unwrap();
        let mut ids: Vec<usize> = (0..concept_count).collect();
        // Shuffle so that ID order differs from generation (and hierarchy) order.
        for index in (1..ids.len()).rev() {
            ids.swap(index, rng.random_range(0..=index));
        }
        let mut concepts: Vec<Concept> = Vec::with_capacity(concept_count);
        for (index, &id) in ids.iter().enumerate() {
            let mut parents = Vec::new();
            if index > 0 {
                for _ in 0..rng.random_range(0..=2) {
                    let parent: &Concept = &concepts[rng.random_range(0..index)];
                    if !parents.contains(&parent.id) {
                        parents.push(parent.id.clone());
                    }
                }
            }
            let synonyms = if rng.random_bool(0.3) {
                vec![format!("syn {id}")]
            } else {
                Vec::new()
            };
            concepts.push(Concept {
                id: format!("c{id:03}"),
                label_fr: format!("Élan {id}"),
                label_en: format!("elk {id}"),
                synonyms,
                parents,
                threshold: rng.random_range(0.05..0.95),
                vector: unit_vector(rng, dim),
            });
        }
        let mut modes = HashMap::new();
        let mut probes = HashMap::new();
        for concept in &concepts {
            let mode = match rng.random_range(0..10) {
                0 => ConceptMode::Inactive,
                1 | 2 => ConceptMode::Classifier,
                _ => ConceptMode::ZeroShot,
            };
            modes.insert(concept.id.clone(), mode);
            probes.insert(
                concept.id.clone(),
                (unit_vector(rng, dim), rng.random_range(0.2..0.9)),
            );
        }
        let mut photos = Vec::with_capacity(photo_count);
        for _ in 0..photo_count {
            // Mix a few Concept vectors with noise so that some Concepts score high.
            let mut embedding: Vec<f32> = unit_vector(rng, dim)
                .into_iter()
                .map(|value| value * rng.random_range(0.1..1.0))
                .collect();
            if concept_count > 0 {
                for _ in 0..rng.random_range(0..4) {
                    let source = &concepts[rng.random_range(0..concept_count)].vector;
                    let weight = rng.random_range(0.2..1.5);
                    for (value, &component) in embedding.iter_mut().zip(source) {
                        *value += weight * component;
                    }
                }
            }
            let embedding = normalized(embedding);
            let mut user_tags = Vec::new();
            for _ in 0..rng.random_range(0..4) {
                let tag = if concept_count == 0 || rng.random_bool(0.2) {
                    format!("vacances {}", rng.random_range(0..5))
                } else {
                    let concept = &concepts[rng.random_range(0..concept_count)];
                    let name = match (rng.random_range(0..3), concept.synonyms.first()) {
                        (0, _) => concept.label_fr.clone(),
                        (1, Some(synonym)) => synonym.clone(),
                        _ => concept.label_en.clone(),
                    };
                    match rng.random_range(0..3) {
                        0 => name.to_uppercase(),
                        1 => format!("  {name} "),
                        _ => name,
                    }
                };
                user_tags.push(tag);
            }
            let mut rejections = Vec::new();
            for _ in 0..rng.random_range(0..4) {
                if concept_count == 0 || rng.random_bool(0.1) {
                    rejections.push("unknown".to_owned());
                } else {
                    rejections.push(concepts[rng.random_range(0..concept_count)].id.clone());
                }
            }
            photos.push(PhotoData {
                embedding,
                user_tags,
                rejections,
                auto_tags_off: rng.random_bool(0.1),
            });
        }
        Self {
            dim,
            model: score_model,
            concepts,
            photos,
            modes,
            probes,
        }
    }

    fn engine(&self) -> AutoTagEngine {
        let vocabulary = Vocabulary::new(self.dim, self.concepts.clone()).unwrap();
        AutoTagEngine::new(vocabulary, self.model).with_classifier(RandomClassifier {
            modes: self.modes.clone(),
            probes: self.probes.clone(),
        })
    }

    fn classifier(&self) -> RandomClassifier {
        RandomClassifier {
            modes: self.modes.clone(),
            probes: self.probes.clone(),
        }
    }
}

struct Borrowed<'a> {
    user_tags: Vec<Vec<&'a str>>,
    rejections: Vec<Vec<&'a str>>,
}

impl<'a> Borrowed<'a> {
    fn new(photos: &'a [PhotoData]) -> Self {
        Self {
            user_tags: photos
                .iter()
                .map(|photo| photo.user_tags.iter().map(String::as_str).collect())
                .collect(),
            rejections: photos
                .iter()
                .map(|photo| photo.rejections.iter().map(String::as_str).collect())
                .collect(),
        }
    }

    fn inputs(&'a self, photos: &'a [PhotoData]) -> Vec<PhotoInput<'a>> {
        photos
            .iter()
            .enumerate()
            .map(|(index, photo)| PhotoInput {
                embedding: &photo.embedding,
                user_tags: &self.user_tags[index],
                rejections: &self.rejections[index],
                auto_tags_off: photo.auto_tags_off,
            })
            .collect()
    }
}

#[derive(Default)]
struct Coverage {
    photos: usize,
    displayed: usize,
    over_cap: usize,
    suppressed: usize,
    implied: usize,
    rejected_would_fire: usize,
    aliased: usize,
    classified_fired: usize,
}

/// Checks every rule for one photo against a reference built from the scenario.
#[expect(clippy::too_many_lines, reason = "one reference model per photo")]
fn check_photo(
    scenario: &Scenario,
    photo: &PhotoData,
    tags: &[AutoTag<'_>],
    coverage: &mut Coverage,
) {
    let by_id: HashMap<&str, &Concept> = scenario
        .concepts
        .iter()
        .map(|concept| (concept.id.as_str(), concept))
        .collect();
    let rejected: HashSet<&str> = photo.rejections.iter().map(String::as_str).collect();
    let user_names: HashSet<String> = photo
        .user_tags
        .iter()
        .map(|tag| normalize_name(tag))
        .collect();
    let aliased: HashSet<&str> = scenario
        .concepts
        .iter()
        .filter(|concept| {
            [&concept.label_fr, &concept.label_en]
                .into_iter()
                .chain(&concept.synonyms)
                .any(|name| user_names.contains(&normalize_name(name)))
        })
        .map(|concept| concept.id.as_str())
        .collect();
    let context = format!("photo {:?}", photo.user_tags);

    // Never more than the cap, never a rejected Concept, never an aliased Concept, sorted.
    let displayed = tags
        .iter()
        .filter(|tag| tag.placement == Placement::Displayed)
        .count();
    assert!(displayed <= MAX_DISPLAYED, "{context}");
    let mut seen = HashSet::new();
    for tag in tags {
        assert!(!rejected.contains(tag.concept), "rejected {}", tag.concept);
        assert!(!aliased.contains(tag.concept), "aliased {}", tag.concept);
        assert!(seen.insert(tag.concept), "duplicate {}", tag.concept);
        assert_eq!(tag.label, by_id[tag.concept].label_fr);
    }
    for pair in tags.windows(2) {
        let order = pair[1]
            .confidence
            .total_cmp(&pair[0].confidence)
            .then(pair[0].concept.cmp(pair[1].concept));
        assert!(order.is_lt(), "{context}: unsorted {pair:?}");
    }
    if photo.auto_tags_off {
        assert!(tags.is_empty(), "{context}: auto tags off");
        return;
    }
    coverage.photos += 1;
    coverage.aliased += aliased.len();

    // Firing: each non-excluded Concept fires exactly when its decision passes, away from
    // rounding-sized ties between the batched and the reference dot products.
    let classifier = scenario.classifier();
    let mut own = HashMap::new();
    for concept in &scenario.concepts {
        let zero_shot = scenario
            .model
            .probability(naive_dot(&concept.vector, &photo.embedding));
        let decision = match scenario.modes[&concept.id] {
            ConceptMode::Inactive => None,
            ConceptMode::ZeroShot => Some(Classification {
                probability: zero_shot,
                threshold: concept.threshold,
            }),
            ConceptMode::Classifier => {
                Some(classifier.decision(concept, &photo.embedding, zero_shot))
            }
        };
        let Some(decision) = decision else { continue };
        let fires = decision.probability >= decision.threshold;
        if rejected.contains(concept.id.as_str()) {
            coverage.rejected_would_fire += usize::from(fires);
            continue;
        }
        if aliased.contains(concept.id.as_str()) {
            continue;
        }
        let fired_tag = tags.iter().find(|tag| {
            tag.concept == concept.id
                && matches!(
                    tag.placement,
                    Placement::Displayed | Placement::OverCap | Placement::SuppressedAncestor
                )
        });
        if (decision.probability - decision.threshold).abs() > 1e-3 {
            assert_eq!(fired_tag.is_some(), fires, "{context}: {}", concept.id);
        }
        if fired_tag.is_some() {
            own.insert(concept.id.as_str(), decision.probability);
            if scenario.modes[&concept.id] == ConceptMode::Classifier {
                coverage.classified_fired += 1;
            }
        }
    }

    // Hierarchy: exactly the non-excluded ancestors of present Concepts are hidden, with the
    // best present descendant's confidence (an aliasing User tag counts as 1).
    let vocabulary = Vocabulary::new(scenario.dim, scenario.concepts.clone()).unwrap();
    let mut expected: HashMap<&str, f32> = HashMap::new();
    let present = own
        .iter()
        .map(|(&id, &probability)| (id, probability))
        .chain(
            aliased
                .iter()
                .filter(|id| !rejected.contains(*id))
                .map(|&id| (id, 1.0)),
        );
    for (id, confidence) in present {
        for ancestor in vocabulary.ancestors(id) {
            let ancestor = ancestor.id.as_str();
            if !rejected.contains(ancestor) && !aliased.contains(ancestor) {
                let best = expected.entry(ancestor).or_insert(0.0);
                *best = best.max(confidence);
            }
        }
    }
    for tag in tags {
        let fired = own.contains_key(tag.concept);
        let ancestor = expected.get(tag.concept);
        match tag.placement {
            Placement::Displayed | Placement::OverCap => {
                assert!(fired && ancestor.is_none(), "{context}: {tag:?}");
            }
            Placement::SuppressedAncestor => assert!(fired && ancestor.is_some(), "{tag:?}"),
            Placement::Implied => assert!(!fired && ancestor.is_some(), "{tag:?}"),
        }
        let reference = ancestor.map_or_else(
            || own[tag.concept],
            |&best| best.max(own.get(tag.concept).copied().unwrap_or(0.0)),
        );
        assert!(
            (tag.confidence - reference).abs() < 1e-4,
            "{context}: {tag:?} expected confidence {reference}"
        );
    }
    for ancestor in expected.keys() {
        assert!(
            seen.contains(ancestor),
            "{context}: missing ancestor {ancestor}"
        );
    }

    // The cap keeps the most confident specific tags.
    let over_cap: Vec<&AutoTag<'_>> = tags
        .iter()
        .filter(|tag| tag.placement == Placement::OverCap)
        .collect();
    if let Some(best_hidden) = over_cap.first() {
        assert_eq!(displayed, MAX_DISPLAYED, "{context}");
        let worst_shown = tags
            .iter()
            .rfind(|tag| tag.placement == Placement::Displayed)
            .unwrap();
        assert!(
            worst_shown.confidence >= best_hidden.confidence,
            "{context}"
        );
    }
    coverage.displayed += displayed;
    coverage.over_cap += over_cap.len();
    coverage.suppressed += tags
        .iter()
        .filter(|tag| tag.placement == Placement::SuppressedAncestor)
        .count();
    coverage.implied += tags
        .iter()
        .filter(|tag| tag.placement == Placement::Implied)
        .count();
}

#[test]
fn random_libraries_obey_every_rule() {
    let mut rng = StdRng::seed_from_u64(0x0A07_07A6);
    let mut coverage = Coverage::default();
    for _ in 0..150 {
        let dim = [4, 8, 16, 33][rng.random_range(0..4)];
        let concept_count = rng.random_range(0..60);
        let photo_count = rng.random_range(1..40);
        let scenario = Scenario::generate(&mut rng, dim, concept_count, photo_count);
        let engine = scenario.engine();
        let borrowed = Borrowed::new(&scenario.photos);
        let inputs = borrowed.inputs(&scenario.photos);
        let library = engine.tag_library(&inputs);
        assert_eq!(library.len(), inputs.len());
        // Deterministic: a second run and every single-photo run give identical results.
        assert_eq!(engine.tag_library(&inputs), library);
        for ((input, photo), result) in inputs.iter().zip(&scenario.photos).zip(&library) {
            assert_eq!(&engine.tag_photo(input), result);
            check_photo(&scenario, photo, result.as_ref().unwrap(), &mut coverage);
        }
    }
    // The generator must actually exercise every rule.
    for (rule, count) in [
        ("scored photos", coverage.photos),
        ("displayed", coverage.displayed),
        ("over cap", coverage.over_cap),
        ("suppressed ancestors", coverage.suppressed),
        ("implied", coverage.implied),
        ("rejections that would fire", coverage.rejected_would_fire),
        ("aliases", coverage.aliased),
        ("classifier fires", coverage.classified_fired),
    ] {
        assert!(count > 20, "{rule} exercised only {count} times");
    }
}

/// Batches cross `sgemm`'s 256-deep k blocks, its row tiles and the 256-photo chunks, so a photo
/// must score identically alone and anywhere in a batch.
#[test]
fn single_photo_matches_any_batch_position() {
    let mut rng = StdRng::seed_from_u64(7);
    let scenario = Scenario::generate(&mut rng, 520, 70, 300);
    let engine = scenario.engine();
    let borrowed = Borrowed::new(&scenario.photos);
    let inputs = borrowed.inputs(&scenario.photos);
    let library = engine.tag_library(&inputs);
    let shifted = engine.tag_library(&inputs[37..]);
    for (index, input) in inputs.iter().enumerate() {
        let single = engine.tag_photo(input);
        assert_eq!(single, library[index], "photo {index}");
        if index >= 37 {
            assert_eq!(single, shifted[index - 37], "shifted photo {index}");
        }
    }
    assert!(library.iter().flatten().map(Vec::len).sum::<usize>() > 100);
}

/// The dot-product prefilter never drops a pair whose computed probability reaches the
/// threshold, including thresholds a few ulps from 0 or 1, and stays within about one logit of
/// the exact boundary.
#[test]
fn dot_cut_is_conservative() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut thresholds = vec![1e-7, 0.001, 0.5, 0.999, 1.0 - 1e-6];
    let mut almost_one = 1.0_f32;
    for _ in 0..8 {
        almost_one = almost_one.next_down();
        thresholds.push(almost_one);
    }
    for round in 0..400 {
        let (scale, bias) = (rng.random_range(0.5..500.0), rng.random_range(-30.0..30.0));
        let model = ScoreModel::new(scale, bias).unwrap();
        let threshold = thresholds
            .get(round)
            .copied()
            .unwrap_or_else(|| rng.random_range(0.0001..0.9999));
        let cut = model.dot_cut(threshold);
        let context = format!("{model:?} threshold {threshold} cut {cut}");
        let mut dot = cut;
        for _ in 0..256 {
            dot = dot.next_down();
            assert!(model.probability(dot) < threshold, "{context}: dot {dot}");
        }
        for _ in 0..1000 {
            let dot = cut - rng.random_range(0.0..0.05);
            assert!(
                dot >= cut || model.probability(dot) < threshold,
                "{context}: dot {dot}"
            );
        }
        let threshold = f64::from(threshold);
        let exact = ((threshold / (1.0 - threshold)).ln() - f64::from(bias)) / f64::from(scale);
        assert!(
            f64::from(cut) > exact - 1.5 / f64::from(scale),
            "{context}: loose"
        );
    }
}

/// Full recompute of a synthetic 10k-photo × 2k-Concept × 768-dimension library.
/// `cargo test --release autotag -- --ignored --nocapture` (set `RAYON_NUM_THREADS` to model a
/// smaller host).
/// The target is defined for release builds, so debug builds do not compile it.
#[cfg(not(debug_assertions))]
#[test]
#[ignore = "release-mode Auto tag recompute benchmark"]
#[expect(clippy::cast_precision_loss, reason = "reporting only")]
fn full_recompute_benchmark() {
    use std::time::{Duration, Instant};

    const PHOTOS: usize = 10_000;
    const CONCEPTS: usize = 2_000;
    const DIM: usize = 768;
    const RUNS: usize = 3;

    let mut rng = StdRng::seed_from_u64(2026);
    let concepts: Vec<Concept> = (0..CONCEPTS)
        .map(|index| Concept {
            id: format!("concept-{index:04}"),
            label_fr: format!("libellé {index}"),
            label_en: format!("label {index}"),
            synonyms: Vec::new(),
            // A forest about four levels deep.
            parents: if index >= 50 {
                vec![format!("concept-{:04}", index / 4)]
            } else {
                Vec::new()
            },
            // With scale 10, bias 0 and random unit vectors (dot std ≈ 0.036), a dot of 0.08
            // (about 2.2 σ, ≈ 28 Concepts per photo) reaches this probability.
            threshold: 1.0 / (1.0 + (-0.8_f32).exp()),
            vector: unit_vector(&mut rng, DIM),
        })
        .collect();
    let vocabulary = Vocabulary::new(DIM, concepts).unwrap();
    let names: Vec<String> = (0..20).map(|index| format!("label {index}")).collect();
    let photos: Vec<PhotoData> = (0..PHOTOS)
        .map(|index| PhotoData {
            embedding: unit_vector(&mut rng, DIM),
            user_tags: vec![names[index % names.len()].clone(), "vacances".to_owned()],
            rejections: vec![format!("concept-{:04}", index % CONCEPTS)],
            auto_tags_off: index % 50 == 0,
        })
        .collect();
    let library_tags: Vec<(&str, usize)> = names.iter().map(|name| (name.as_str(), 500)).collect();
    let aliases = AliasIndex::build(&vocabulary, library_tags);
    let engine =
        AutoTagEngine::new(vocabulary, ScoreModel::new(10.0, 0.0).unwrap()).with_aliases(&aliases);
    let borrowed = Borrowed::new(&photos);
    let inputs = borrowed.inputs(&photos);

    std::hint::black_box(engine.tag_library(&inputs[..2048]));
    let mut timings = Vec::with_capacity(RUNS);
    let mut library = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        library = engine.tag_library(&inputs);
        timings.push(started.elapsed());
    }
    let started = Instant::now();
    let samples = 200;
    // Offset by one: every 50th photo has auto tags off and skips scoring.
    for input in inputs.iter().skip(1).step_by(PHOTOS / samples) {
        std::hint::black_box(engine.tag_photo(input).unwrap());
    }
    let single = started.elapsed() / u32::try_from(samples).unwrap();

    let tags: Vec<&Vec<AutoTag<'_>>> = library
        .iter()
        .map(|result| result.as_ref().unwrap())
        .collect();
    let count = |placement| {
        tags.iter()
            .flat_map(|tags| tags.iter())
            .filter(|tag| tag.placement == placement)
            .count() as f64
            / PHOTOS as f64
    };
    println!(
        "Auto tag recompute: {PHOTOS} photos × {CONCEPTS} Concepts × {DIM} dims on {} threads: \
         {timings:?}; single photo {single:?}; per photo: {:.1} displayed, {:.1} over cap, \
         {:.1} suppressed, {:.1} implied",
        rayon::current_num_threads(),
        count(Placement::Displayed),
        count(Placement::OverCap),
        count(Placement::SuppressedAncestor),
        count(Placement::Implied),
    );
    // Single-photo and batch paths agree at production scale (crossing sgemm's 1024-column
    // blocks as well).
    for index in (0..PHOTOS).step_by(97).chain([PHOTOS - 1]) {
        assert_eq!(engine.tag_photo(&inputs[index]).as_ref(), Ok(tags[index]));
    }
    assert!(
        count(Placement::Displayed) > 1.0,
        "the benchmark must fire Concepts"
    );
    let slowest = timings.iter().max().unwrap();
    assert!(
        *slowest < Duration::from_secs(1),
        "full recompute took {slowest:?}"
    );
}
