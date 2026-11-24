//! Explicit public-only fixture replay; never invoked by ordinary tests or HTTP.
use super::{Examples, Logistic};
use serde::Deserialize;

#[derive(Deserialize)]
struct PublicFixture {
    training: Vec<Vec<(Vec<f32>, bool)>>,
    validation: Vec<(Vec<f32>, bool)>,
    independent: Vec<(Vec<f32>, bool)>,
}

#[test]
#[ignore = "explicit public Open Images fixture; no owner library data"]
fn public_snow_feedback_quality() {
    let path = std::env::var("UROCISSA_PUBLIC_FEEDBACK_FIXTURE")
        .expect("explicit public input fixture is required");
    let fixture: PublicFixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(fixture.training.len(), 5);
    for (seed, training) in fixture.training.iter().enumerate() {
        let examples = training
            .iter()
            .map(|(v, y)| (v.as_slice(), 0.0, *y))
            .collect::<Vec<_>>();
        let classifier = Logistic::fit(&examples, 768);
        let positives = training
            .iter()
            .filter(|e| e.1)
            .map(|e| e.0.as_slice())
            .collect::<Vec<_>>();
        let negatives = training
            .iter()
            .filter(|e| !e.1)
            .map(|e| e.0.as_slice())
            .collect::<Vec<_>>();
        assert_eq!((positives.len(), negatives.len()), (30, 6));
        let baseline = Examples::fit(&positives, &negatives).unwrap();
        for (name, test) in [
            ("validation", &fixture.validation),
            ("independent", &fixture.independent),
        ] {
            let mut old = [0_usize; 4];
            let mut new = [0_usize; 4];
            for (vector, label) in test {
                for (predicted, counts) in [
                    (baseline.probability(vector) >= 0.5, &mut old),
                    (classifier.probability(vector, 0.0) >= 0.8, &mut new),
                ] {
                    let index = match (predicted, *label) {
                        (true, true) => 0,
                        (false, true) => 1,
                        (true, false) => 2,
                        (false, false) => 3,
                    };
                    counts[index] += 1;
                }
            }
            let [tp, fn_, fp, tn] = new;
            assert!(tp + fn_ >= 5 && fp + tn >= 5 && tp + fp >= 5);
            // Integer forms of observed precision>=.9, recall>=.1 and equal-prior>=.9.
            assert!(10 * tp >= 9 * (tp + fp));
            assert!(10 * tp >= tp + fn_);
            assert!(tp * (fp + tn) >= 9 * fp * (tp + fn_));
            eprintln!("snow seed{seed} {name} old/new TP,FN,FP,TN: {old:?} {new:?}");
        }
    }
}
