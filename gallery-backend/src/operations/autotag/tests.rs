//! Table-driven tests for each engine rule on a small hand-made vocabulary.
use super::*;

/// Six Concept axes plus a padding axis that keeps embeddings normalized.
const DIM: usize = 7;
const PAD: usize = 6;
const ANIMAL: usize = 0;
const BIRD: usize = 1;
const STORK: usize = 2;
const DOG: usize = 3;
const BEACH: usize = 4;
const SUNSET: usize = 5;

fn axis(dim: usize, index: usize) -> Vec<f32> {
    let mut vector = vec![0.0; dim];
    vector[index] = 1.0;
    vector
}

fn concept(id: &str, fr: &str, en: &str, parents: &[&str], axis_index: usize) -> Concept {
    Concept {
        id: id.to_owned(),
        label_fr: fr.to_owned(),
        label_en: en.to_owned(),
        synonyms: Vec::new(),
        parents: parents.iter().map(|&parent| parent.to_owned()).collect(),
        threshold: 0.5,
        vector: axis(DIM, axis_index),
    }
}

fn fixture_concepts() -> Vec<Concept> {
    let mut stork = concept("stork", "cigogne", "stork", &["bird"], STORK);
    stork.synonyms.push("white stork".to_owned());
    vec![
        concept("sunset", "coucher de soleil", "sunset", &[], SUNSET),
        stork,
        concept("dog", "chien", "dog", &["animal"], DOG),
        concept("bird", "oiseau", "bird", &["animal"], BIRD),
        concept("beach", "plage", "beach", &[], BEACH),
        concept("animal", "animal", "animal", &[], ANIMAL),
    ]
}

fn fixture() -> Vocabulary {
    Vocabulary::new(DIM, fixture_concepts()).unwrap()
}

/// With one-hot Concept vectors, a Concept's dot product is exactly its embedding component.
fn model() -> ScoreModel {
    ScoreModel::new(10.0, -5.0).unwrap()
}

fn p(dot: f32) -> f32 {
    model().probability(dot)
}

fn embedding(dim: usize, components: &[(usize, f32)]) -> Vec<f32> {
    let mut vector = vec![0.0; dim];
    for &(index, value) in components {
        vector[index] = value;
    }
    let rest = 1.0 - vector.iter().map(|value| value * value).sum::<f32>();
    assert!(
        rest >= 0.0,
        "components {components:?} exceed a unit vector"
    );
    vector[dim - 1] = rest.max(0.0).sqrt();
    vector
}

fn summary<'e>(tags: &[AutoTag<'e>]) -> Vec<(&'e str, Placement, f32)> {
    tags.iter()
        .map(|tag| (tag.concept, tag.placement, tag.confidence))
        .collect()
}

struct Case {
    name: &'static str,
    components: &'static [(usize, f32)],
    user_tags: &'static [&'static str],
    rejections: &'static [&'static str],
    auto_tags_off: bool,
    expected: Vec<(&'static str, Placement, f32)>,
}

impl Case {
    fn new(name: &'static str, components: &'static [(usize, f32)]) -> Self {
        Self {
            name,
            components,
            user_tags: &[],
            rejections: &[],
            auto_tags_off: false,
            expected: Vec::new(),
        }
    }

    fn tags(mut self, user_tags: &'static [&'static str]) -> Self {
        self.user_tags = user_tags;
        self
    }

    fn reject(mut self, rejections: &'static [&'static str]) -> Self {
        self.rejections = rejections;
        self
    }

    fn off(mut self) -> Self {
        self.auto_tags_off = true;
        self
    }

    fn expect(mut self, expected: &[(&'static str, Placement, f32)]) -> Self {
        self.expected = expected.to_vec();
        self
    }

    fn run(&self, engine: &AutoTagEngine) {
        let embedding = embedding(DIM, self.components);
        let photo = PhotoInput {
            embedding: &embedding,
            user_tags: self.user_tags,
            rejections: self.rejections,
            auto_tags_off: self.auto_tags_off,
        };
        let tags = engine.tag_photo(&photo).unwrap();
        assert_eq!(summary(&tags), self.expected, "{}", self.name);
    }
}

#[test]
fn rules_table() {
    use Placement::{Displayed, Implied, SuppressedAncestor};
    let (p6, p7) = (p(0.6), p(0.7));
    let engine = AutoTagEngine::new(fixture(), model());
    let cases = [
        // Scoring and hierarchy: search expansion through implied ancestors.
        Case::new("fires at threshold", &[(STORK, 0.7)]).expect(&[
            ("animal", Implied, p7),
            ("bird", Implied, p7),
            ("stork", Displayed, p7),
        ]),
        Case::new("below threshold", &[(STORK, 0.45)]),
        Case::new("fired ancestor is suppressed", &[(STORK, 0.7), (BIRD, 0.6)]).expect(&[
            ("animal", Implied, p7),
            ("bird", SuppressedAncestor, p7),
            ("stork", Displayed, p7),
        ]),
        Case::new(
            "ancestor keeps its own higher score",
            &[(STORK, 0.6), (BIRD, 0.7)],
        )
        .expect(&[
            ("animal", Implied, p7),
            ("bird", SuppressedAncestor, p7),
            ("stork", Displayed, p6),
        ]),
        Case::new(
            "two branches share an ancestor",
            &[(STORK, 0.7), (DOG, 0.6)],
        )
        .expect(&[
            ("animal", Implied, p7),
            ("bird", Implied, p7),
            ("stork", Displayed, p7),
            ("dog", Displayed, p6),
        ]),
        Case::new("independent Concepts", &[(BEACH, 0.6), (SUNSET, 0.7)])
            .expect(&[("sunset", Displayed, p7), ("beach", Displayed, p6)]),
        // Rejection always wins.
        Case::new("rejection beats a high score", &[(STORK, 0.95)]).reject(&["stork"]),
        Case::new("rejected implied ancestor", &[(STORK, 0.7)])
            .reject(&["bird"])
            .expect(&[("animal", Implied, p7), ("stork", Displayed, p7)]),
        Case::new("rejected fired ancestor", &[(STORK, 0.7), (BIRD, 0.6)])
            .reject(&["bird"])
            .expect(&[("animal", Implied, p7), ("stork", Displayed, p7)]),
        Case::new("rejection beats an aliasing User tag", &[])
            .tags(&["cigogne"])
            .reject(&["stork"]),
        Case::new("unknown rejection is ignored", &[(BEACH, 0.7)])
            .reject(&["unicorn"])
            .expect(&[("beach", Displayed, p7)]),
        // Alias versus Auto tag.
        Case::new("alias replaces the Auto tag", &[(STORK, 0.9)])
            .tags(&[" Cigogne "])
            .expect(&[("animal", Implied, 1.0), ("bird", Implied, 1.0)]),
        Case::new("alias by English label", &[])
            .tags(&["STORK"])
            .expect(&[("animal", Implied, 1.0), ("bird", Implied, 1.0)]),
        Case::new("alias by synonym", &[])
            .tags(&["White Stork"])
            .expect(&[("animal", Implied, 1.0), ("bird", Implied, 1.0)]),
        Case::new("aliased ancestor needs no Auto tag", &[(STORK, 0.7)])
            .tags(&["oiseau"])
            .expect(&[("animal", Implied, 1.0), ("stork", Displayed, p7)]),
        Case::new("aliased leaf suppresses its fired ancestor", &[(BIRD, 0.7)])
            .tags(&["cigogne"])
            .expect(&[("animal", Implied, 1.0), ("bird", SuppressedAncestor, 1.0)]),
        Case::new("unrelated User tag", &[(BEACH, 0.7)])
            .tags(&["vacances"])
            .expect(&[("beach", Displayed, p7)]),
        // Auto-tags-off.
        Case::new("auto tags off", &[(STORK, 0.7), (BEACH, 0.7)]).off(),
        Case::new("auto tags off beats an alias", &[])
            .tags(&["cigogne"])
            .off(),
    ];
    for case in &cases {
        case.run(&engine);
    }
}

#[derive(Default)]
struct Scripted {
    modes: Vec<(&'static str, ConceptMode)>,
    /// `None` echoes the zero-shot probability with a threshold of 0.
    decisions: Vec<(&'static str, Option<Classification>)>,
}

impl ConceptClassifier for Scripted {
    fn mode(&self, concept: &Concept) -> ConceptMode {
        self.modes
            .iter()
            .find(|(id, _)| *id == concept.id)
            .map_or(ConceptMode::ZeroShot, |&(_, mode)| mode)
    }

    fn classify(&self, concept: &Concept, embedding: &[f32], zero_shot: f32) -> Classification {
        assert_eq!(embedding.len(), DIM);
        let decision = self
            .decisions
            .iter()
            .find(|(id, _)| *id == concept.id)
            .and_then(|&(_, decision)| decision);
        decision.unwrap_or(Classification {
            probability: zero_shot,
            threshold: 0.0,
        })
    }
}

#[test]
fn classifier_hook_table() {
    use ConceptMode::{Classifier, Inactive};
    use Placement::{Displayed, Implied};
    let p7 = p(0.7);
    let decide = |probability, threshold| {
        Some(Classification {
            probability,
            threshold,
        })
    };
    let cases = [
        (
            Scripted {
                modes: vec![("stork", Inactive)],
                ..Scripted::default()
            },
            Case::new("inactive Concept never fires", &[(STORK, 0.95)]),
        ),
        (
            Scripted {
                modes: vec![("stork", Inactive)],
                ..Scripted::default()
            },
            Case::new("inactive Concept still aliases", &[])
                .tags(&["cigogne"])
                .expect(&[("animal", Implied, 1.0), ("bird", Implied, 1.0)]),
        ),
        (
            Scripted {
                modes: vec![("bird", Inactive)],
                ..Scripted::default()
            },
            Case::new(
                "inactive ancestor is still implied",
                &[(STORK, 0.7), (BIRD, 0.7)],
            )
            .expect(&[
                ("animal", Implied, p7),
                ("bird", Implied, p7),
                ("stork", Displayed, p7),
            ]),
        ),
        (
            Scripted {
                modes: vec![("beach", Classifier)],
                decisions: vec![("beach", decide(0.97, 0.9))],
            },
            Case::new("classifier replaces the probability", &[])
                .expect(&[("beach", Displayed, 0.97)]),
        ),
        (
            Scripted {
                modes: vec![("beach", Classifier)],
                decisions: vec![("beach", decide(0.97, 0.99))],
            },
            Case::new("classifier replaces the threshold", &[(BEACH, 0.9)]),
        ),
        (
            Scripted {
                modes: vec![("beach", Classifier)],
                decisions: vec![("beach", decide(f32::NAN, 0.0))],
            },
            Case::new("NaN classifier probability never fires", &[(BEACH, 0.9)]),
        ),
        (
            Scripted {
                modes: vec![("sunset", Classifier)],
                decisions: vec![("sunset", None)],
            },
            Case::new(
                "classifier receives the zero-shot probability",
                &[(SUNSET, 0.3)],
            )
            .expect(&[("sunset", Displayed, p(0.3))]),
        ),
        (
            Scripted {
                modes: vec![("beach", Classifier)],
                decisions: vec![("beach", decide(0.97, 0.1))],
            },
            Case::new("classifier cannot override a Rejection", &[(BEACH, 0.9)]).reject(&["beach"]),
        ),
        (
            Scripted {
                modes: vec![("beach", Classifier)],
                decisions: vec![("beach", decide(0.97, 0.1))],
            },
            Case::new("classifier cannot override an alias", &[(BEACH, 0.9)]).tags(&["plage"]),
        ),
    ];
    for (classifier, case) in cases {
        let engine = AutoTagEngine::new(fixture(), model()).with_classifier(classifier);
        case.run(&engine);
    }
}

#[test]
fn cap_keeps_eight_most_confident_specific_tags() {
    use Placement::{Displayed, OverCap, SuppressedAncestor};
    // Components per ID; ID order is the reverse of confidence order, and c03/c04 tie.
    let components = [
        ("c00", 0.10),
        ("c01", 0.12),
        ("c02", 0.14),
        ("c03", 0.16),
        ("c04", 0.16),
        ("c05", 0.18),
        ("c06", 0.20),
        ("c07", 0.22),
        ("c08", 0.24),
        ("c09", 0.26),
        ("c10", 0.28),
        ("c11", 0.30),
        ("root", 0.35),
    ];
    let dim = components.len() + 1;
    let concepts = components
        .iter()
        .enumerate()
        .map(|(index, &(id, _))| Concept {
            id: id.to_owned(),
            label_fr: format!("fr {id}"),
            label_en: format!("en {id}"),
            synonyms: Vec::new(),
            parents: if id == "c00" {
                vec!["root".to_owned()]
            } else {
                Vec::new()
            },
            threshold: 0.6,
            vector: axis(dim, index),
        })
        .collect();
    let model = ScoreModel::new(10.0, 0.0).unwrap();
    let engine = AutoTagEngine::new(Vocabulary::new(dim, concepts).unwrap(), model);
    let embedding = embedding(
        dim,
        &components
            .iter()
            .enumerate()
            .map(|(index, &(_, value))| (index, value))
            .collect::<Vec<_>>(),
    );
    let p = |id: &str| model.probability(components.iter().find(|c| c.0 == id).unwrap().1);
    let expected = |rejected: &str, displayed: &[&'static str], over_cap: &[&'static str]| {
        let mut expected = vec![("root", SuppressedAncestor, p("root"))];
        expected.extend(displayed.iter().map(|&id| (id, Displayed, p(id))));
        expected.extend(over_cap.iter().map(|&id| (id, OverCap, p(id))));
        expected.retain(|tag| tag.0 != rejected);
        expected
    };
    let cases = [
        (
            "the ancestor takes no chip and the ID breaks the tie",
            "",
            expected(
                "",
                &["c11", "c10", "c09", "c08", "c07", "c06", "c05", "c03"],
                &["c04", "c02", "c01", "c00"],
            ),
        ),
        (
            "a Rejection frees a chip",
            "c11",
            expected(
                "c11",
                &["c10", "c09", "c08", "c07", "c06", "c05", "c03", "c04"],
                &["c02", "c01", "c00"],
            ),
        ),
    ];
    for (name, rejected, expected) in cases {
        let photo = PhotoInput {
            embedding: &embedding,
            user_tags: &[],
            rejections: &[rejected],
            auto_tags_off: false,
        };
        let tags = engine.tag_photo(&photo).unwrap();
        assert_eq!(summary(&tags), expected, "{name}");
        let displayed = tags.iter().filter(|tag| tag.placement == Displayed).count();
        assert_eq!(displayed, MAX_DISPLAYED, "{name}");
    }
}

#[test]
fn threshold_is_inclusive() {
    // Bias 0 and an orthogonal embedding give a probability of exactly 0.5.
    let model = ScoreModel::new(10.0, 0.0).unwrap();
    for (threshold, fires) in [(0.5, true), (0.5_f32.next_up(), false)] {
        let mut beach = concept("beach", "plage", "beach", &[], BEACH);
        beach.threshold = threshold;
        let engine = AutoTagEngine::new(Vocabulary::new(DIM, vec![beach]).unwrap(), model);
        let embedding = axis(DIM, PAD);
        let photo = PhotoInput {
            embedding: &embedding,
            user_tags: &[],
            rejections: &[],
            auto_tags_off: false,
        };
        let tags = engine.tag_photo(&photo).unwrap();
        assert_eq!(tags.len(), usize::from(fires), "threshold {threshold}");
    }
}

type EmbeddingCase = (&'static str, Vec<f32>, bool, Result<(), VectorProblem>);

#[test]
fn invalid_embeddings_table() {
    use VectorProblem::{DimensionMismatch, NonFinite, NotNormalized};
    let engine = AutoTagEngine::new(fixture(), model());
    let mut nan = embedding(DIM, &[]);
    nan[BEACH] = f32::NAN;
    let mut infinite = embedding(DIM, &[]);
    infinite[BEACH] = f32::INFINITY;
    let mut slightly_long = embedding(DIM, &[]);
    slightly_long[PAD] = 1.0 + NORM_TOLERANCE / 2.0;
    let cases: [EmbeddingCase; 8] = [
        (
            "too short",
            vec![0.0, 1.0, 0.0],
            false,
            Err(DimensionMismatch {
                expected: DIM,
                actual: 3,
            }),
        ),
        (
            "empty",
            Vec::new(),
            false,
            Err(DimensionMismatch {
                expected: DIM,
                actual: 0,
            }),
        ),
        (
            "too long",
            vec![0.0; DIM + 1],
            false,
            Err(DimensionMismatch {
                expected: DIM,
                actual: DIM + 1,
            }),
        ),
        (
            "auto tags off still validates",
            vec![1.0],
            true,
            Err(DimensionMismatch {
                expected: DIM,
                actual: 1,
            }),
        ),
        ("NaN", nan, false, Err(NonFinite)),
        ("infinite", infinite, false, Err(NonFinite)),
        (
            "not normalized",
            vec![1.0; DIM],
            false,
            Err(NotNormalized {
                norm: 7.0_f32.sqrt(),
            }),
        ),
        ("within tolerance", slightly_long, false, Ok(())),
    ];
    for (name, embedding, auto_tags_off, expected) in cases {
        let photo = PhotoInput {
            embedding: &embedding,
            user_tags: &[],
            rejections: &[],
            auto_tags_off,
        };
        let result = engine.tag_photo(&photo).map(|_| ());
        assert_eq!(
            result,
            expected.map_err(AutoTagError::InvalidEmbedding),
            "{name}"
        );
    }
}

#[test]
fn invalid_embedding_fails_only_its_photo() {
    let engine = AutoTagEngine::new(fixture(), model());
    let good = embedding(DIM, &[(BEACH, 0.7)]);
    let short = vec![1.0; DIM - 1];
    let photos = [&good, &short, &good].map(|embedding| PhotoInput {
        embedding,
        user_tags: &[],
        rejections: &[],
        auto_tags_off: false,
    });
    let results = engine.tag_library(&photos);
    assert_eq!(results.len(), 3);
    assert_eq!(
        results[1],
        Err(AutoTagError::InvalidEmbedding(
            VectorProblem::DimensionMismatch {
                expected: DIM,
                actual: DIM - 1,
            }
        ))
    );
    let beach = vec![("beach", Placement::Displayed, p(0.7))];
    assert_eq!(summary(results[0].as_ref().unwrap()), beach);
    assert_eq!(summary(results[2].as_ref().unwrap()), beach);
}

#[test]
#[expect(clippy::too_many_lines, reason = "one row per validation rule")]
fn vocabulary_validation_table() {
    type Edit = fn(&mut Vec<Concept>);
    fn case(name: &str, edit: Edit, expected: AutoTagError) -> (&str, Edit, AutoTagError) {
        (name, edit, expected)
    }
    fn position(concepts: &[Concept], id: &str) -> usize {
        concepts
            .iter()
            .position(|concept| concept.id == id)
            .unwrap()
    }
    let error = |name: &str| AutoTagError::EmptyName {
        concept: name.to_owned(),
    };
    let cases = [
        case(
            "short Concept vector",
            |c| {
                let i = position(c, "dog");
                c[i].vector.pop();
            },
            AutoTagError::InvalidConceptVector {
                concept: "dog".to_owned(),
                problem: VectorProblem::DimensionMismatch {
                    expected: DIM,
                    actual: DIM - 1,
                },
            },
        ),
        case(
            "unnormalized Concept vector",
            |c| {
                let i = position(c, "dog");
                c[i].vector[0] = 1.0;
            },
            AutoTagError::InvalidConceptVector {
                concept: "dog".to_owned(),
                problem: VectorProblem::NotNormalized {
                    norm: 2.0_f32.sqrt(),
                },
            },
        ),
        case(
            "duplicate ID",
            |c| {
                let mut copy = c[0].clone();
                copy.label_fr = "autre".to_owned();
                copy.label_en = "other".to_owned();
                c.push(copy);
            },
            AutoTagError::DuplicateConcept {
                concept: "sunset".to_owned(),
            },
        ),
        case(
            "blank French label",
            |c| {
                let i = position(c, "dog");
                c[i].label_fr = "  ".to_owned();
            },
            error("dog"),
        ),
        case(
            "blank synonym",
            |c| {
                let i = position(c, "beach");
                c[i].synonyms.push(String::new());
            },
            error("beach"),
        ),
        case("blank ID", |c| c[0].id = " ".to_owned(), error(" ")),
        case(
            "name shared after normalization",
            |c| {
                let mut puppy = concept("puppy", "chiot", " Chien", &["dog"], DOG);
                puppy.vector = axis(DIM, PAD);
                c.push(puppy);
            },
            AutoTagError::AmbiguousName {
                name: "chien".to_owned(),
                first: "dog".to_owned(),
                second: "puppy".to_owned(),
            },
        ),
        case(
            "zero threshold",
            |c| {
                let i = position(c, "dog");
                c[i].threshold = 0.0;
            },
            AutoTagError::InvalidThreshold {
                concept: "dog".to_owned(),
                threshold: 0.0,
            },
        ),
        case(
            "threshold of one",
            |c| {
                let i = position(c, "dog");
                c[i].threshold = 1.0;
            },
            AutoTagError::InvalidThreshold {
                concept: "dog".to_owned(),
                threshold: 1.0,
            },
        ),
        case(
            "unknown parent",
            |c| {
                let i = position(c, "dog");
                c[i].parents.push("mammal".to_owned());
            },
            AutoTagError::UnknownParent {
                concept: "dog".to_owned(),
                parent: "mammal".to_owned(),
            },
        ),
        case(
            "self parent",
            |c| {
                let i = position(c, "beach");
                c[i].parents.push("beach".to_owned());
            },
            AutoTagError::HierarchyCycle {
                concept: "beach".to_owned(),
            },
        ),
        case(
            "longer cycle",
            |c| {
                let i = position(c, "animal");
                c[i].parents.push("stork".to_owned());
            },
            AutoTagError::HierarchyCycle {
                concept: "animal".to_owned(),
            },
        ),
    ];
    for (name, edit, expected) in cases {
        let mut concepts = fixture_concepts();
        edit(&mut concepts);
        assert_eq!(
            Vocabulary::new(DIM, concepts).unwrap_err(),
            expected,
            "{name}"
        );
    }
    let mut nan_threshold = fixture_concepts();
    nan_threshold[0].threshold = f32::NAN;
    assert!(matches!(
        Vocabulary::new(DIM, nan_threshold),
        Err(AutoTagError::InvalidThreshold { .. })
    ));
    assert_eq!(
        Vocabulary::new(0, Vec::new()).unwrap_err(),
        AutoTagError::ZeroDimension
    );
    assert!(
        Vocabulary::new(DIM, Vec::new())
            .unwrap()
            .concepts()
            .is_empty()
    );
}

#[test]
fn score_model_validation_table() {
    for (scale, bias, valid) in [
        (117.3, -12.9, true),
        (1.0, 0.0, true),
        (0.0, 0.0, false),
        (-1.0, 0.0, false),
        (f32::NAN, 0.0, false),
        (f32::INFINITY, 0.0, false),
        (1.0, f32::NAN, false),
        (1.0, f32::NEG_INFINITY, false),
    ] {
        assert_eq!(
            ScoreModel::new(scale, bias).is_ok(),
            valid,
            "{scale} {bias}"
        );
    }
    assert!((ScoreModel::new(1.0, 0.0).unwrap().probability(0.0) - 0.5).abs() < f32::EPSILON);
}

#[test]
fn alias_index_picks_most_used_then_lexicographic() {
    let vocabulary = fixture();
    let library = [
        ("cigogne", 3),
        ("Stork", 5),
        ("vacances", 10),
        ("chien", 2),
        ("Chien", 2),
        ("plage", 0),
    ];
    let forward = AliasIndex::build(&vocabulary, library);
    let backward = AliasIndex::build(&vocabulary, library.into_iter().rev());
    assert_eq!(forward, backward, "input order must not matter");
    for (concept, alias) in [
        ("stork", Some("Stork")),
        ("dog", Some("Chien")),
        ("beach", None),
        ("bird", None),
    ] {
        assert_eq!(forward.alias(concept), alias, "{concept}");
    }
    assert_eq!(AliasIndex::default().alias("stork"), None);
}

#[test]
fn display_label_is_alias_or_french() {
    let vocabulary = fixture();
    let aliases = AliasIndex::build(&vocabulary, [("Stork", 5), ("toutou", 9)]);
    let engine = AutoTagEngine::new(vocabulary, model());
    let embedding = embedding(DIM, &[(STORK, 0.7), (BEACH, 0.6)]);
    let photo = PhotoInput {
        embedding: &embedding,
        user_tags: &[],
        rejections: &[],
        auto_tags_off: false,
    };
    let labels = |engine: &AutoTagEngine| -> Vec<(String, String)> {
        engine
            .tag_photo(&photo)
            .unwrap()
            .iter()
            .map(|tag| (tag.concept.to_owned(), tag.label.to_owned()))
            .collect()
    };
    let french: Vec<(String, String)> = [
        ("animal", "animal"),
        ("bird", "oiseau"),
        ("stork", "cigogne"),
        ("beach", "plage"),
    ]
    .map(|(concept, label)| (concept.to_owned(), label.to_owned()))
    .to_vec();
    assert_eq!(labels(&engine), french);
    let engine = engine.with_aliases(&aliases);
    let mut relabeled = french.clone();
    relabeled[2].1 = "Stork".to_owned();
    assert_eq!(labels(&engine), relabeled);
    // Rebuilding with no aliases restores the French labels.
    let engine = engine.with_aliases(&AliasIndex::default());
    assert_eq!(labels(&engine), french);
}

#[test]
fn vocabulary_lookups() {
    let vocabulary = fixture();
    assert_eq!(vocabulary.dim(), DIM);
    let ids: Vec<&str> = vocabulary
        .concepts()
        .iter()
        .map(|concept| concept.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["animal", "beach", "bird", "dog", "stork", "sunset"],
        "sorted by ID"
    );
    assert_eq!(vocabulary.concept("dog").unwrap().label_fr, "chien");
    assert!(vocabulary.concept("Dog").is_none(), "IDs match exactly");
    for name in ["cigogne", "  CIGOGNE ", "Stork", "white stork"] {
        assert_eq!(
            vocabulary.concept_named(name).unwrap().id,
            "stork",
            "{name}"
        );
    }
    assert!(vocabulary.concept_named("cigognes").is_none());
    let ancestors: Vec<&str> = vocabulary
        .ancestors("stork")
        .map(|concept| concept.id.as_str())
        .collect();
    assert_eq!(ancestors, ["animal", "bird"]);
    assert_eq!(vocabulary.ancestors("unicorn").count(), 0);
    assert_eq!(normalize_name("  Écureuil ROUX "), "écureuil roux");

    let engine = AutoTagEngine::new(vocabulary, model());
    assert_eq!(engine.vocabulary().concepts().len(), 6);
}

#[test]
fn errors_display_their_subject() {
    let error = AutoTagError::InvalidEmbedding(VectorProblem::DimensionMismatch {
        expected: 768,
        actual: 512,
    });
    assert_eq!(
        error.to_string(),
        "photo embedding expected 768 dimensions, got 512"
    );
    let error = AutoTagError::UnknownParent {
        concept: "stork".to_owned(),
        parent: "wader".to_owned(),
    };
    assert_eq!(
        error.to_string(),
        "Concept `stork` has unknown parent `wader`"
    );
    assert!(check_vector(&axis(3, 0), 3).is_ok());
}

#[test]
fn default_engine_is_zero_shot() {
    let embedding = embedding(DIM, &[(STORK, 0.7), (BEACH, 0.55)]);
    let photo = PhotoInput {
        embedding: &embedding,
        user_tags: &["oiseau"],
        rejections: &[],
        auto_tags_off: false,
    };
    let default = AutoTagEngine::new(fixture(), model());
    let explicit = AutoTagEngine::new(fixture(), model()).with_classifier(ZeroShot);
    let result: PhotoResult<'_> = default.tag_photo(&photo);
    assert_eq!(result, explicit.tag_photo(&photo));
    assert_eq!(result.unwrap().len(), 3);
}
