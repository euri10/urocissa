use super::*;
use crate::operations::autotag::AutoTagEngine;

fn base() -> Vocabulary {
    Vocabulary::new(
        3,
        vec![Concept {
            id: "cat".into(),
            label: "cat".into(),
            synonyms: vec![],
            parents: vec![],
            threshold: 0.9,
            vector: vec![1.0, 0.0, 0.0],
        }],
    )
    .unwrap()
}

fn photo<'a>(
    embedding: &'a [f32],
    user_tags: &'a [&'a str],
    rejections: &'a [&'a str],
) -> PhotoInput<'a> {
    PhotoInput {
        embedding,
        user_tags,
        rejections,
        auto_tags_off: false,
    }
}

#[test]
fn personal_gate_counts_distinct_photos_and_builtin_aliases_never_make_personal_concepts() {
    let ids = (0..5).map(|i| i.to_string()).collect::<Vec<_>>();
    let input = photo(&[1.0, 0.0, 0.0], &["Moka", " moka ", "CAT"], &[]);
    let examples = ids
        .iter()
        .map(|id| (id.as_str(), input))
        .collect::<Vec<_>>();
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    let personal = personal_id("moka");
    for count in [4, 5] {
        let mut examples = examples[..count].to_vec();
        examples.push(examples[0]); // duplicates are not an additional positive.
        let (vocabulary, learning) =
            Learning::fit(&base(), BTreeSet::from(["cat".into()]), score, &examples).unwrap();
        assert_eq!(vocabulary.concepts().len(), 2);
        assert!(learning.classifiers.is_empty());
        let engine = AutoTagEngine::new(vocabulary, score).with_classifier(learning);
        let result = engine
            .tag_photo(&photo(&[1.0, 0.0, 0.0], &[], &[]))
            .unwrap();
        assert_eq!(result.iter().any(|tag| tag.concept == personal), count == 5);
        let rejected = [&*personal];
        assert!(
            engine
                .tag_photo(&photo(&[1.0, 0.0, 0.0], &[], &rejected))
                .unwrap()
                .iter()
                .all(|tag| tag.concept != personal)
        );
        assert!(engine.tag_photo(&input).unwrap().is_empty());
    }
    assert_eq!(personal_id(" MOKA "), personal);
}

#[test]
fn promoted_personal_negatives_count_once_and_win_over_contradictory_tags() {
    let legacy = personal_id("CAT");
    let rejects = [legacy.as_str(), "cat"];
    let positive = photo(&[1.0, 0.0, 0.0], &["cat"], &[]);
    let negative = photo(&[0.0, 0.0, 1.0], &["cat"], &rejects);
    let ids = (0..10).map(|i| i.to_string()).collect::<Vec<_>>();
    let photos = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), if i < 5 { positive } else { negative }))
        .collect::<Vec<_>>();
    let (_, learning) = Learning::fit(
        &base(),
        BTreeSet::from(["cat".into()]),
        ScoreModel::new(10.0, -5.0).unwrap(),
        &photos,
    )
    .unwrap();
    assert_eq!(learning.method("cat"), "learned");
    let (_, learning) = Learning::fit(
        &base(),
        BTreeSet::from(["cat".into()]),
        ScoreModel::new(10.0, -5.0).unwrap(),
        &photos[..9],
    )
    .unwrap();
    assert_eq!(
        learning.method("cat"),
        "builtin",
        "equivalent rejection keys are one negative photo"
    );
}

#[test]
fn classifier_gate_requires_five_explicit_positives_and_negatives() {
    let ids = (0..10).map(|i| i.to_string()).collect::<Vec<_>>();
    let positive = photo(&[1.0, 0.0, 0.0], &["Cat", "cat"], &[]);
    let negative = photo(&[0.0, 0.0, 1.0], &[], &["cat"]);
    for (positives, negatives) in [(4, 5), (5, 4), (5, 5)] {
        let mut examples = ids[..positives]
            .iter()
            .map(|id| (id.as_str(), positive))
            .collect::<Vec<_>>();
        examples.extend(
            ids[5..5 + negatives]
                .iter()
                .map(|id| (id.as_str(), negative)),
        );
        let (vocabulary, learning) = Learning::fit(
            &base(),
            BTreeSet::from(["cat".into()]),
            ScoreModel::new(10.0, -5.0).unwrap(),
            &examples,
        )
        .unwrap();
        assert_eq!(
            learning.mode(vocabulary.concept("cat").unwrap()),
            if positives == 5 && negatives == 5 {
                ConceptMode::Classifier
            } else {
                ConceptMode::ZeroShot
            }
        );
    }
    // Plenty of untagged photos provide no negative labels.
    let examples = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                id.as_str(),
                if i < 5 {
                    positive
                } else {
                    photo(&[0.0, 0.0, 1.0], &[], &[])
                },
            )
        })
        .collect::<Vec<_>>();
    let (_, learning) = Learning::fit(
        &base(),
        BTreeSet::new(),
        ScoreModel::new(10.0, -5.0).unwrap(),
        &examples,
    )
    .unwrap();
    assert!(learning.classifiers.is_empty());
}

#[test]
fn only_validated_snow_uses_feedback_only_classifier_after_five_by_five_gate() {
    let concepts = ["snow", "building"]
        .into_iter()
        .map(|id| Concept {
            id: id.into(),
            label: id.into(),
            synonyms: vec![],
            parents: vec![],
            threshold: 0.5,
            vector: vec![0.0, 1.0, 0.0],
        })
        .collect();
    let vocabulary = Vocabulary::new(3, concepts).unwrap();
    let ids = (0..10).map(|i| i.to_string()).collect::<Vec<_>>();
    let positive = photo(&[1.0, 0.0, 0.0], &["snow", "building"], &[]);
    let negative = photo(&[-1.0, 0.0, 0.0], &[], &["snow", "building"]);
    for (positives, negatives, expected) in
        [(4, 5, "inactive"), (5, 4, "similarity"), (5, 5, "learned")]
    {
        let examples = ids[..positives]
            .iter()
            .map(|id| (id.as_str(), positive))
            .chain(
                ids[5..5 + negatives]
                    .iter()
                    .map(|id| (id.as_str(), negative)),
            )
            .collect::<Vec<_>>();
        let (vocabulary, learning) = Learning::fit(
            &vocabulary,
            BTreeSet::new(),
            ScoreModel::new(10.0, -5.0).unwrap(),
            &examples,
        )
        .unwrap();
        assert_eq!(learning.method("snow"), expected);
        assert_eq!(
            learning.method("building"),
            if positives >= 5 {
                "similarity"
            } else {
                "inactive"
            }
        );
        if expected == "learned" {
            assert_eq!(
                learning.classifiers["snow"].zero_shot_weight.to_bits(),
                0.0_f32.to_bits()
            );
            let engine = AutoTagEngine::new(vocabulary, ScoreModel::new(10.0, -5.0).unwrap())
                .with_classifier(learning);
            let result = engine
                .tag_photo(&photo(&[1.0, 0.0, 0.0], &[], &["snow"]))
                .unwrap();
            assert!(result.iter().all(|tag| tag.concept != "snow"));
            assert!(result.iter().any(|tag| tag.concept == "building"));
        }
    }
}

/// Unit vector at `angle` radians in the (y, z) plane: cosines between two are cos(Δangle).
fn at(angle: f32) -> [f32; 3] {
    [0.0, angle.cos(), angle.sin()]
}

fn fires(engine: &AutoTagEngine, concept: &str, embedding: &[f32], rejections: &[&str]) -> bool {
    engine
        .tag_photo(&photo(embedding, &[], rejections))
        .unwrap()
        .iter()
        .any(|tag| tag.concept == concept)
}

#[test]
fn multimodal_personal_concept_matches_each_cluster_deterministically() {
    let ids = (0..10).map(|i| i.to_string()).collect::<Vec<_>>();
    let id = personal_id("Moka");
    let rejected = [&*id];
    let positive_a = photo(&[0.6, 0.8, 0.0], &["Moka"], &[]);
    let positive_b = photo(&[0.6, -0.8, 0.0], &["Moka"], &[]);
    let negative = photo(&[0.0, 0.0, 1.0], &[], &rejected);
    let mut examples = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            (
                id.as_str(),
                if i < 3 {
                    positive_a
                } else if i < 5 {
                    positive_b
                } else {
                    negative
                },
            )
        })
        .collect::<Vec<_>>();
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    // The examples' mean (axis 0) is near neither cluster; nearest-example matching finds both.
    for examples in [&examples[..5], &examples[..]] {
        let (vocabulary, learning) =
            Learning::fit(&base(), BTreeSet::new(), score, examples).unwrap();
        assert_eq!(learning.method(&id), "similarity");
        let engine = AutoTagEngine::new(vocabulary, score).with_classifier(learning);
        for embedding in [positive_a.embedding, positive_b.embedding] {
            assert!(fires(&engine, &id, embedding, &[]));
            assert!(!fires(&engine, &id, embedding, &rejected));
        }
        assert!(!fires(&engine, &id, negative.embedding, &[]));
        assert!(!fires(&engine, &id, &[1.0, 0.0, 0.0], &[]));
    }
    let (vocabulary, learning) = Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
    examples.reverse();
    let (rebuilt, relearned) = Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
    assert_eq!(learning, relearned);
    assert_eq!(vocabulary.concepts(), rebuilt.concepts());
}

#[test]
fn uncalibrated_builtin_activates_from_owner_positives_alone() {
    let ids = (0..6).map(|i| i.to_string()).collect::<Vec<_>>();
    let embeddings = [-0.2, -0.1, 0.0, 0.1, 0.2].map(at);
    let tagged = embeddings
        .iter()
        .map(|embedding| photo(embedding, &["cat"], &[]))
        .collect::<Vec<_>>();
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    // Within the examples' own 0.1 rad spacing of one of them, and well outside it.
    let similar = at(0.25);
    let different = at(0.6);
    for count in [4, 5] {
        let examples = ids[..count]
            .iter()
            .zip(&tagged)
            .map(|(id, photo)| (id.as_str(), *photo))
            .collect::<Vec<_>>();
        // Uncalibrated (not active): never a zero-shot guess, so no Rejection can ever exist.
        let (vocabulary, learning) =
            Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
        assert_eq!(
            learning.method("cat"),
            if count == 5 { "similarity" } else { "inactive" }
        );
        assert!(learning.classifiers.is_empty());
        let engine = AutoTagEngine::new(vocabulary, score).with_classifier(learning);
        assert_eq!(fires(&engine, "cat", &similar, &[]), count == 5);
        assert!(!fires(&engine, "cat", &different, &[]));
        assert!(
            !fires(&engine, "cat", &[1.0, 0.0, 0.0], &[]),
            "the prompt is ignored"
        );
        assert!(
            !fires(&engine, "cat", &similar, &["cat"]),
            "a Rejection always wins"
        );
        // A tagged photo shows its User tag, not a duplicate Auto tag.
        assert!(engine.tag_photo(&tagged[0]).unwrap().is_empty());
    }
    // A calibrated built-in keeps its prompt; owner positives alone do not replace it.
    let examples = ids[..5]
        .iter()
        .zip(&tagged)
        .map(|(id, photo)| (id.as_str(), *photo))
        .collect::<Vec<_>>();
    let (_, learning) =
        Learning::fit(&base(), BTreeSet::from(["cat".into()]), score, &examples).unwrap();
    assert_eq!(learning.method("cat"), "builtin");
}

#[test]
fn every_rejection_tightens_example_matching() {
    let ids = (0..7).map(|i| i.to_string()).collect::<Vec<_>>();
    // Spread examples: a loose 0.3 rad gap at the edge, 0.1 rad elsewhere.
    let positives = [-0.6, -0.3, -0.2, -0.1, 0.0].map(at);
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    let fit = |rejections: &[[f32; 3]]| {
        let mut examples = positives
            .iter()
            .map(|embedding| photo(embedding, &["cat"], &[]))
            .collect::<Vec<_>>();
        examples.extend(
            rejections
                .iter()
                .map(|embedding| photo(embedding, &[], &["cat"])),
        );
        let examples = ids
            .iter()
            .map(String::as_str)
            .zip(examples)
            .collect::<Vec<_>>();
        let (vocabulary, learning) =
            Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
        assert_eq!(learning.method("cat"), "similarity");
        AutoTagEngine::new(vocabulary, score).with_classifier(learning)
    };
    let near = at(0.05);
    let looser = at(0.2);
    let opposite = at(-0.8);
    // No Rejection: the loosest example's 0.3 rad gap is the 10% spread quantile.
    let engine = fit(&[]);
    assert!(fires(&engine, "cat", &near, &[]));
    assert!(fires(&engine, "cat", &looser, &[]));
    assert!(fires(&engine, "cat", &opposite, &[]));
    // Rejecting a photo 0.15 rad from the examples raises the bar everywhere to that distance,
    // and vetoes anything closer to it than to an example.
    let engine = fit(&[at(0.15)]);
    assert!(fires(&engine, "cat", &near, &[]));
    assert!(
        !fires(&engine, "cat", &looser, &[]),
        "closer to the Rejection"
    );
    assert!(
        !fires(&engine, "cat", &opposite, &[]),
        "below the raised bar"
    );
    assert!(!fires(&engine, "cat", &at(0.15), &[]));
    // Two Rejections tighten it further, deterministically.
    let engine = fit(&[at(0.15), at(-0.65)]);
    assert!(fires(&engine, "cat", &at(0.02), &[]));
    assert!(!fires(&engine, "cat", &at(0.08), &[]));
    assert!(!fires(&engine, "cat", &at(0.12), &[]));
}

#[test]
fn fast_example_decisions_match_complete_scores_at_rounding_boundaries() {
    let deltas = [
        -0.1,
        -f32::EPSILON,
        -f32::MIN_POSITIVE,
        0.0,
        f32::MIN_POSITIVE,
        f32::EPSILON,
        0.1,
    ];
    for positive in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        for cutoff_delta in deltas {
            for rejection_delta in deltas {
                for rejections in [
                    vec![],
                    vec![vec![positive + rejection_delta]],
                    vec![vec![-1.0], vec![positive + rejection_delta]],
                ] {
                    let example = Examples {
                        positives: vec![vec![positive]],
                        rejections,
                        cutoff: positive + cutoff_delta,
                    };
                    let full = example.probability(&[1.0]);
                    let fast = example.firing_probability(&[1.0]);
                    assert_eq!(fast.is_some(), full >= 0.5);
                    if let Some(fast) = fast {
                        assert_eq!(fast.to_bits(), full.to_bits());
                    }
                }
            }
        }
    }
    // A mathematically negative margin can round to exactly 0.5 and still fire.
    let rounding = Examples {
        positives: vec![vec![0.0]],
        rejections: vec![],
        cutoff: f32::MIN_POSITIVE,
    };
    assert_eq!(rounding.probability(&[1.0]).to_bits(), 0.5_f32.to_bits());
    assert_eq!(
        rounding.firing_probability(&[1.0]).map(f32::to_bits),
        Some(0.5_f32.to_bits())
    );
}

#[test]
fn fast_example_decisions_match_full_scores_for_random_vectors() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    let mut rng = StdRng::seed_from_u64(0x51_6c_69_70);
    for _ in 0..2_000 {
        let query: Vec<f32> = (0..32).map(|_| rng.random_range(-1.0..1.0)).collect();
        let positive_count = rng.random_range(1..9);
        let rejection_count = rng.random_range(0..9);
        let mut vector = || (0..32).map(|_| rng.random_range(-1.0..1.0)).collect();
        let positives = (0..positive_count).map(|_| vector()).collect();
        let rejections = (0..rejection_count).map(|_| vector()).collect();
        let example = Examples {
            positives,
            rejections,
            cutoff: rng.random_range(-4.0..4.0),
        };
        let full = example.probability(&query);
        let fast = example.firing_probability(&query);
        assert_eq!(fast.is_some(), full >= 0.5);
        if let Some(fast) = fast {
            assert_eq!(fast.to_bits(), full.to_bits());
        }
    }
}

#[test]
fn profile_dot_upper_bound_covers_exact_dot() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    let mut rng = StdRng::seed_from_u64(0x51_6c_69_70);
    for scale in [1e-25_f32, 1e-10, 1.0, 1e10, 1e18] {
        for _ in 0..500 {
            let left = (0..768)
                .map(|_| rng.random_range(-1.0..1.0) * scale)
                .collect::<Vec<_>>();
            let right = (0..768)
                .map(|_| rng.random_range(-1.0..1.0) / scale / 768.0)
                .collect::<Vec<_>>();
            let upper = dot_upper_bound(&left, &right).unwrap();
            assert!(upper >= dot(&left, &right));
        }
    }
    assert!(dot_upper_bound(&[1.0], &[1.0]).is_none());
    assert!(dot_upper_bound(&[0.1; 768], &[0.1; 768]).is_none());
    assert!(dot_upper_bound(&[f32::MAX; 768], &[f32::MAX; 768]).is_none());
}

#[test]
fn bounded_precheck_preserves_example_decisions_near_cutoff() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    let mut rng = StdRng::seed_from_u64(0x70_72_6f_6f_66);
    for _ in 0..200 {
        let mut vector = || {
            let mut values = (0..768)
                .map(|_| rng.random_range(-1.0..1.0))
                .collect::<Vec<f32>>();
            let norm = dot(&values, &values).sqrt();
            for value in &mut values {
                *value /= norm;
            }
            values
        };
        let query = vector();
        let positives = (0..5).map(|_| vector()).collect::<Vec<_>>();
        let rejections = (0..3).map(|_| vector()).collect::<Vec<_>>();
        let positive = nearest(&positives, &query, None);
        for offset in [-0.01, -0.001, -f32::EPSILON, 0.0, f32::EPSILON, 0.001, 0.01] {
            let example = Examples {
                positives: positives.clone(),
                rejections: rejections.clone(),
                cutoff: positive + offset,
            };
            let actual = example.firing_probability(&query);
            let full = example.probability(&query);
            assert_eq!(actual.is_some(), full >= 0.5);
            if let Some(value) = actual {
                assert_eq!(value.to_bits(), full.to_bits());
            }
        }
    }
    let left = (0..768)
        .map(|index| if index % 2 == 0 { 1e18_f32 } else { -1e18 })
        .collect::<Vec<_>>();
    let right = vec![1e-21_f32; 768];
    assert!(dot_upper_bound(&left, &right).unwrap() >= dot(&left, &right));

    // Non-finite intermediate arithmetic must use the original exact path.
    let extreme = Examples {
        positives: vec![vec![f32::MAX; 768]],
        rejections: vec![],
        cutoff: 0.0,
    };
    let query = [f32::MAX; 768];
    assert_eq!(
        extreme.firing_probability(&query).map(f32::to_bits),
        Some(extreme.probability(&query).to_bits())
    );
}

#[test]
#[ignore = "synthetic release-profile bounded similarity attribution"]
fn bounded_similarity_profile() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};
    use std::{hint::black_box, time::Instant};

    let mut rng = StdRng::seed_from_u64(23);
    let vectors = (0..1_100)
        .map(|_| {
            let mut vector = (0..768)
                .map(|_| rng.random_range(-1.0..1.0))
                .collect::<Vec<f32>>();
            let norm = dot(&vector, &vector).sqrt();
            for value in &mut vector {
                *value /= norm;
            }
            vector
        })
        .collect::<Vec<_>>();
    for round in 0..=5 {
        for bounded in [false, true] {
            let start = Instant::now();
            for query in &vectors[..1_100] {
                for positive in &vectors[..156] {
                    if bounded {
                        black_box(dot_upper_bound(query, positive).unwrap());
                    } else {
                        black_box(dot(query, positive));
                    }
                }
            }
            eprintln!(
                "{} round={round} ms={:.3}",
                if bounded { "bounded" } else { "exact" },
                start.elapsed().as_secs_f64() * 1_000.0
            );
        }
    }
}

#[test]
#[ignore = "synthetic release-profile similarity attribution"]
fn similarity_hotspot_profile() {
    use rand::{RngExt, SeedableRng, rngs::StdRng};
    use std::time::{Duration, Instant};

    let queries = std::env::var("UROCISSA_PROFILE_QUERIES")
        .unwrap_or_else(|_| "1100".into())
        .parse::<usize>()
        .unwrap();
    let samples = std::env::var("UROCISSA_PROFILE_SAMPLES")
        .unwrap_or_else(|_| "5".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=1_100).contains(&queries) && samples > 0);
    let mut rng = StdRng::seed_from_u64(23);
    let vectors: Vec<Vec<f32>> = (0..1_100)
        .map(|_| {
            let mut vector: Vec<f32> = (0..768).map(|_| rng.random_range(-1.0..1.0)).collect();
            let norm = dot(&vector, &vector).sqrt();
            for value in &mut vector {
                *value /= norm;
            }
            vector
        })
        .collect();
    let examples: Vec<Examples> = [(29, 8, 0), (68, 9, 100), (59, 29, 200)]
        .into_iter()
        .map(|(positive_count, rejection_count, offset)| {
            let positives = vectors[offset..offset + positive_count]
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>();
            let rejections = vectors
                [offset + positive_count..offset + positive_count + rejection_count]
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>();
            Examples::fit(&positives, &rejections).unwrap()
        })
        .collect();
    for sample in 1..=samples {
        let (mut positive_time, mut rejection_time, mut skippable_time) =
            (Duration::ZERO, Duration::ZERO, Duration::ZERO);
        let (mut skippable, mut fired) = (0, 0);
        for example in &examples {
            for vector in &vectors[..queries] {
                let start = Instant::now();
                let positive = nearest(&example.positives, vector, None);
                positive_time += start.elapsed();
                let start = Instant::now();
                let rejection = nearest(&example.rejections, vector, None);
                let elapsed = start.elapsed();
                rejection_time += elapsed;
                if sigmoid(MARGIN_SLOPE * (positive - example.cutoff)) < 0.5 {
                    skippable += 1;
                    skippable_time += elapsed;
                }
                let margin = (positive - example.cutoff).min(positive - rejection);
                fired += usize::from(sigmoid(MARGIN_SLOPE * margin) >= 0.5);
            }
        }
        eprintln!(
            "sample={sample} positives_ms={:.3} rejections_ms={:.3} skippable_rejections_ms={:.3} skippable={skippable}/{} fired={fired}",
            positive_time.as_secs_f64() * 1000.0,
            rejection_time.as_secs_f64() * 1000.0,
            skippable_time.as_secs_f64() * 1000.0,
            queries * examples.len(),
        );
    }
}
