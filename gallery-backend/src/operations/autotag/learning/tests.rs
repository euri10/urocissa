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

#[test]
fn multimodal_personal_concept_learns_beyond_its_prototype_deterministically() {
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
    let (prototype_vocab, prototype) =
        Learning::fit(&base(), BTreeSet::new(), score, &examples[..5]).unwrap();
    let prototype_engine = AutoTagEngine::new(prototype_vocab, score).with_classifier(prototype);
    let target = photo(positive_b.embedding, &[], &[]);
    assert!(prototype_engine.tag_photo(&target).unwrap().is_empty());
    let (vocabulary, learning) = Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
    assert!(learning.classifiers.contains_key(&id));
    examples.reverse();
    let (rebuilt, relearned) = Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
    assert_eq!(learning, relearned);
    assert_eq!(vocabulary.concepts(), rebuilt.concepts());
    let engine = AutoTagEngine::new(vocabulary, score).with_classifier(learning);
    for embedding in [positive_a.embedding, positive_b.embedding] {
        assert!(
            engine
                .tag_photo(&photo(embedding, &[], &[]))
                .unwrap()
                .iter()
                .any(|t| t.concept == id)
        );
        assert!(
            engine
                .tag_photo(&photo(embedding, &[], &rejected))
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        engine
            .tag_photo(&photo(negative.embedding, &[], &[]))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn uncalibrated_builtin_activates_from_owner_positives_alone() {
    let ids = (0..6).map(|i| i.to_string()).collect::<Vec<_>>();
    let tagged = photo(&[0.0, 1.0, 0.0], &["cat"], &[]);
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    let similar = [0.0, 0.98, 0.198_997_5];
    let different = [0.0, 0.0, 1.0];
    let fires = |engine: &AutoTagEngine, embedding: &[f32], rejections: &[&str]| {
        engine
            .tag_photo(&photo(embedding, &[], rejections))
            .unwrap()
            .iter()
            .any(|tag| tag.concept == "cat")
    };
    for count in [4, 5] {
        let examples = ids[..count]
            .iter()
            .map(|id| (id.as_str(), tagged))
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
        assert_eq!(fires(&engine, &similar, &[]), count == 5);
        assert!(!fires(&engine, &different, &[]));
        assert!(
            !fires(&engine, &similar, &["cat"]),
            "a Rejection always wins"
        );
        // A tagged photo shows its User tag, not a duplicate Auto tag.
        assert!(engine.tag_photo(&tagged).unwrap().is_empty());
    }
    // A calibrated built-in keeps its prompt; owner positives alone do not replace it.
    let examples = ids[..5]
        .iter()
        .map(|id| (id.as_str(), tagged))
        .collect::<Vec<_>>();
    let (_, learning) =
        Learning::fit(&base(), BTreeSet::from(["cat".into()]), score, &examples).unwrap();
    assert_eq!(learning.method("cat"), "builtin");
}

#[test]
fn activated_builtin_learns_finer_distinctions_from_rejections() {
    let ids = (0..10).map(|i| i.to_string()).collect::<Vec<_>>();
    let positive = photo(&[0.0, 1.0, 0.0], &["cat"], &[]);
    let negative = photo(&[0.0, 0.6, 0.8], &[], &["cat"]);
    let examples = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), if i < 5 { positive } else { negative }))
        .collect::<Vec<_>>();
    let score = ScoreModel::new(10.0, -5.0).unwrap();
    let (vocabulary, learning) = Learning::fit(&base(), BTreeSet::new(), score, &examples).unwrap();
    assert_eq!(learning.method("cat"), "learned");
    let engine = AutoTagEngine::new(vocabulary, score).with_classifier(learning);
    let fires = |embedding: &[f32]| {
        engine
            .tag_photo(&photo(embedding, &[], &[]))
            .unwrap()
            .iter()
            .any(|tag| tag.concept == "cat")
    };
    // The prompt vector (axis 0) is irrelevant: only the owner's examples decide.
    assert!(fires(positive.embedding));
    assert!(!fires(negative.embedding));
    assert!(!fires(&[1.0, 0.0, 0.0]));
}
