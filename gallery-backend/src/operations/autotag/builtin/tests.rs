use std::collections::HashMap;

use crate::operations::autotag::{
    BuiltinCatalog, BuiltinConcept, PhotoInput, Placement, ScoreModel,
};

fn source() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../../assets/concepts.json")).unwrap()
}

#[test]
fn embedded_catalog_is_english_only() {
    let catalog = BuiltinCatalog::embedded().unwrap();
    let labels: HashMap<_, _> = catalog
        .concepts
        .iter()
        .map(|c: &BuiltinConcept| (c.id.as_str(), c.label_en.as_str()))
        .collect();
    assert_eq!(labels["birthday"], "birthday party");
    assert_eq!(catalog.prompts()[0], "a photo of person");
    let vocabulary = catalog.alias_vocabulary().unwrap();
    assert_eq!(vocabulary.concept("sunset").unwrap().label, "sunset");
    assert_eq!(vocabulary.concept_named("Bike").unwrap().id, "bicycle");
    for french in [
        "coucher de soleil",
        "cigogne",
        "oiseau",
        "vélo",
        "bicyclette",
    ] {
        assert!(vocabulary.concept_named(french).is_none(), "{french}");
    }
}

#[test]
fn committed_report_and_calibrated_thresholds_match() {
    let catalog = BuiltinCatalog::embedded().unwrap();
    let report: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/concepts/calibration-report.json"
    )))
    .unwrap();
    assert_eq!(report["model"]["model_id"], catalog.model_id);
    if report["vocabulary_version"] != catalog.version.as_str() {
        // A display-only relabel keeps the calibrated prompts and thresholds; the Python
        // artifact test proves the scoring definitions are unchanged.
        let relabel: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tools/concepts/relabel.json"
        )))
        .unwrap();
        assert_eq!(
            relabel["calibrated_vocabulary_version"],
            report["vocabulary_version"]
        );
        assert_eq!(relabel["vocabulary_version"], catalog.version.as_str());
    }
    assert_eq!(
        report["concepts"].as_object().unwrap().len(),
        catalog.concepts.len()
    );
    for concept in catalog.concepts {
        let result = &report["concepts"][&concept.id];
        let threshold: Option<f32> = serde_json::from_value(result["threshold"].clone()).unwrap();
        assert_eq!(threshold, concept.threshold, "{}", concept.id);
        assert_eq!(result["status"] == "calibrated", threshold.is_some());
    }
}

#[test]
fn excluded_attributes_cannot_enter_the_builtin_catalog() {
    for attribute in [
        "man",
        "woman",
        "gender",
        "ethnicity",
        "religion",
        "orientation",
        "disability",
        "happy",
        "sad",
        "angry",
        "smiling",
        "thin",
        "attractive",
        "body",
    ] {
        let mut value = source();
        value["concepts"][0]["id"] = attribute.into();
        value["concepts"][0]["label_en"] = attribute.into();
        assert!(
            BuiltinCatalog::from_json(&value.to_string())
                .unwrap_err()
                .to_string()
                .contains("exclusion policy")
        );
    }
    // Reusing a safe ID cannot smuggle an excluded English scoring label through.
    let mut value = source();
    value["concepts"][0]["label_en"] = "woman".into();
    assert!(BuiltinCatalog::from_json(&value.to_string()).is_err());
}

#[test]
fn malformed_catalog_reuses_engine_validation() {
    let mutations: &[fn(&mut serde_json::Value)] = &[
        |v| v["schema_version"] = 1.into(),
        |v| v["version"] = "".into(),
        |v| v["concepts"][0]["label_en"] = "".into(),
        // French labels cannot come back in through the catalog.
        |v| v["concepts"][0]["label_fr"] = "personne".into(),
        |v| v["concepts"][1]["id"] = "person".into(),
        |v| v["concepts"][0]["threshold"] = 1.into(),
        |v| v["concepts"][0]["parents"] = serde_json::json!(["missing"]),
        |v| v["concepts"][0]["parents"] = serde_json::json!(["crowd"]),
    ];
    for mutate in mutations {
        let mut value = source();
        mutate(&mut value);
        assert!(BuiltinCatalog::from_json(&value.to_string()).is_err());
    }
}

fn fixture() -> BuiltinCatalog {
    BuiltinCatalog::from_json(
        r#"{
        "schema_version":2,"version":"fixture","model_id":"fixture-model",
        "prompt_template":"a photo of {label}","concepts":[
        {"id":"animal","label_en":"animal","parents":[],"threshold":null,"open_images":"Animal"},
        {"id":"bird","label_en":"bird","parents":["animal"],"threshold":null,"open_images":"Bird"},
        {"id":"stork","label_en":"stork","parents":["bird"],"threshold":0.8,"open_images":"Stork"}
        ]}"#,
    )
    .unwrap()
}

fn vectors() -> HashMap<String, Vec<f32>> {
    ["animal", "bird", "stork"]
        .into_iter()
        .map(|id| (id.to_owned(), vec![1.0]))
        .collect()
}

#[test]
fn dropped_concepts_never_fire_but_still_expand_parents() {
    let engine = fixture()
        .into_engine(
            "fixture-model",
            1,
            vectors(),
            ScoreModel::new(10.0, 0.0).unwrap(),
        )
        .unwrap();
    let tags = engine
        .tag_photo(&PhotoInput {
            embedding: &[1.0],
            user_tags: &[],
            rejections: &[],
            auto_tags_off: false,
        })
        .unwrap();
    assert_eq!(
        tags.iter()
            .filter(|t| t.placement == Placement::Displayed)
            .count(),
        1
    );
    assert!(
        tags.iter()
            .any(|t| t.concept == "bird" && t.placement == Placement::Implied)
    );
    let mut catalog = fixture();
    catalog.concepts[2].threshold = None;
    let engine = catalog
        .into_engine(
            "fixture-model",
            1,
            vectors(),
            ScoreModel::new(10.0, 0.0).unwrap(),
        )
        .unwrap();
    assert!(
        engine
            .tag_photo(&PhotoInput {
                embedding: &[1.0],
                user_tags: &[],
                rejections: &[],
                auto_tags_off: false,
            })
            .unwrap()
            .is_empty()
    );
}

#[test]
fn wrong_model_missing_vectors_and_bad_vectors_are_refused() {
    let model = ScoreModel::new(10.0, 0.0).unwrap();
    assert!(
        fixture()
            .into_engine("other-model", 1, vectors(), model)
            .is_err()
    );
    let mut missing = vectors();
    missing.remove("bird");
    assert!(
        fixture()
            .into_engine("fixture-model", 1, missing, model)
            .is_err()
    );
    let mut invalid = vectors();
    invalid.insert("bird".into(), vec![0.0]);
    assert!(
        fixture()
            .into_engine("fixture-model", 1, invalid, model)
            .is_err()
    );
}
