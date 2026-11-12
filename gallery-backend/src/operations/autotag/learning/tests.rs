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
