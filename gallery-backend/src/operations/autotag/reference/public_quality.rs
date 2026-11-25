//! Opt-in replay through the actual engine, with public pixels/vectors left in scratch.
use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::operations::autotag::{BuiltinCatalog, ScoreModel, check_vector};

#[derive(Deserialize)]
struct PublicFixture {
    model_id: String,
    vocabulary_version: String,
    scale: f32,
    bias: f32,
    prompts: HashMap<String, Vec<f32>>,
    cases: BTreeMap<String, Case>,
}

#[derive(Deserialize)]
struct Case {
    expected: [usize; 4], // TP,FN,FP,TN
    photos: Vec<(Vec<f32>, bool)>,
}

#[test]
#[ignore = "explicit public frozen fixture; never owner-library data"]
fn public_default_heldout_quality() {
    let path = std::env::var("UROCISSA_PUBLIC_DEFAULTS_FIXTURE")
        .expect("explicit public fixture is required");
    let fixture: PublicFixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    // Replay the original frozen release, including quarantined heads, so the
    // original per-Concept gates remain checked without reactivating them.
    let defaults: super::ReferenceDefaults =
        serde_json::from_str(include_str!("../../../../assets/concept-defaults.json")).unwrap();
    assert_eq!(fixture.vocabulary_version, defaults.vocabulary_version);
    let mut catalog = BuiltinCatalog::embedded().unwrap();
    catalog.references = defaults.heads;
    let engine = catalog
        .into_engine(
            &fixture.model_id,
            768,
            fixture.prompts,
            ScoreModel::new(fixture.scale, fixture.bias).unwrap(),
        )
        .unwrap();
    assert_eq!(fixture.cases.len(), 8);
    for (name, case) in fixture.cases {
        let concept = engine.vocabulary().concept(&name).unwrap();
        let mut counts = [0_usize; 4];
        for (vector, label) in case.photos {
            check_vector(&vector, 768).unwrap();
            let (_, decision) = engine.explain_score(concept, &vector);
            let decision = decision.expect("qualified default is active");
            let predicted = decision.probability >= decision.threshold;
            counts[match (predicted, label) {
                (true, true) => 0,
                (false, true) => 1,
                (true, false) => 2,
                (false, false) => 3,
            }] += 1;
        }
        assert_eq!(
            counts, case.expected,
            "{name}: native/public evaluator prediction disagreement"
        );
        let [tp, fn_, fp, tn] = counts;
        assert!(tp + fn_ >= 5 && fp + tn >= 5 && tp + fp >= 5);
        assert!(10 * tp >= 9 * (tp + fp), "{name}: observed precision");
        assert!(10 * tp >= tp + fn_, "{name}: recall");
        assert!(
            tp * (fp + tn) >= 9 * fp * (tp + fn_),
            "{name}: equal-prior precision"
        );
        eprintln!("public default {name} TP,FN,FP,TN: {counts:?}");
    }
}
