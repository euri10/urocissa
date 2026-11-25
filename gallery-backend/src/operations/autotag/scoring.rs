//! Zero-shot scoring: validated vectors, the `SigLIP`-style sigmoid and the batched dot products.
use super::error::{AutoTagError, VectorProblem};

/// An embedding or Concept vector norm may differ from 1 by at most this much.
pub const NORM_TOLERANCE: f32 = 1e-3;

/// The model's calibration of a dot product into a probability:
/// `sigmoid(logit_scale * dot + logit_bias)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreModel {
    logit_scale: f32,
    logit_bias: f32,
}

impl ScoreModel {
    /// `logit_scale` is the multiplier itself (the exponential of a `SigLIP` checkpoint's stored
    /// `logit_scale` parameter), so it must be positive.
    pub fn new(logit_scale: f32, logit_bias: f32) -> Result<Self, AutoTagError> {
        if logit_scale.is_finite() && logit_scale > 0.0 && logit_bias.is_finite() {
            Ok(Self {
                logit_scale,
                logit_bias,
            })
        } else {
            Err(AutoTagError::InvalidScoreModel {
                logit_scale,
                logit_bias,
            })
        }
    }

    /// The zero-shot probability of a Concept given its dot product with a photo embedding.
    /// Both scoring paths call exactly this function, so they agree to the bit.
    pub fn probability(self, dot: f32) -> f32 {
        let logit = self.logit_scale * dot + self.logit_bias;
        1.0 / (1.0 + (-logit).exp())
    }

    /// A conservative lower bound on the dot product needed to reach `threshold`.
    ///
    /// Scoring compares `dot >= cut` first and evaluates [`Self::probability`] only for those
    /// candidates, so the cut must never exclude a dot whose computed probability reaches the
    /// threshold. One full logit of margin (plus a rounding allowance for large scales) is far
    /// wider than any `f32` rounding in the sigmoid, even for thresholds one ulp below 1.
    pub(super) fn dot_cut(self, threshold: f32) -> f32 {
        let threshold = f64::from(threshold);
        let scale = f64::from(self.logit_scale);
        let bias = f64::from(self.logit_bias);
        let logit = (threshold / (1.0 - threshold)).ln();
        let margin = 1.0 + (scale + bias.abs()) * 1e-5;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "rounding is covered by the margin"
        )]
        let cut = ((logit - bias - margin) / scale) as f32;
        cut
    }
}

/// Checks a vector's dimension, finiteness and L2 normalization.
pub fn check_vector(vector: &[f32], dim: usize) -> Result<(), VectorProblem> {
    if vector.len() != dim {
        return Err(VectorProblem::DimensionMismatch {
            expected: dim,
            actual: vector.len(),
        });
    }
    if !vector.iter().all(|value| value.is_finite()) {
        return Err(VectorProblem::NonFinite);
    }
    let norm = vector
        .iter()
        .map(|&value| f64::from(value) * f64::from(value))
        .sum::<f64>()
        .sqrt();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "only reported and compared"
    )]
    let norm = norm as f32;
    if (norm - 1.0).abs() <= NORM_TOLERANCE {
        Ok(())
    } else {
        Err(VectorProblem::NotNormalized { norm })
    }
}

/// Row-major `rows × concepts` dot products between `rows` packed embeddings and the packed
/// Concept matrix (`concepts × dim`, row-major).
///
/// `sgemm` accumulates every output element over `dim` in a fixed order that depends on neither
/// `rows` nor the element's position in a tile, so a photo scores identically alone and inside
/// any batch (verified by the single-versus-batch property test).
pub(super) fn dot_products(
    embeddings: &[f32],
    rows: usize,
    matrix: &[f32],
    concepts: usize,
    dim: usize,
) -> Vec<f32> {
    assert_eq!(embeddings.len(), rows * dim, "embedding block shape");
    assert_eq!(matrix.len(), concepts * dim, "Concept matrix shape");
    let mut dots = vec![0.0; rows * concepts];
    if dots.is_empty() || dim == 0 {
        return dots;
    }
    let dim_stride = isize::try_from(dim).expect("dimension fits isize");
    let concept_stride = isize::try_from(concepts).expect("Concept count fits isize");
    // SAFETY: A is `rows × dim` with row stride `dim` and column stride 1, exactly the
    // `embeddings` slice. B is `dim × concepts` read through the row-major Concept matrix with
    // row stride 1 and column stride `dim`, exactly the `matrix` slice. C is `rows × concepts`
    // with row stride `concepts` and column stride 1, exactly the exclusively borrowed `dots`.
    // The shapes are asserted above and the slices do not overlap.
    unsafe {
        matrixmultiply::sgemm(
            rows,
            dim,
            concepts,
            1.0,
            embeddings.as_ptr(),
            dim_stride,
            1,
            matrix.as_ptr(),
            1,
            dim_stride,
            0.0,
            dots.as_mut_ptr(),
            concept_stride,
            1,
        );
    }
    dots
}

#[cfg(test)]
mod matrix_tests {
    use super::dot_products;

    const DIM: usize = 768;
    const CONCEPTS: usize = 82;

    fn values(len: usize, seed: usize) -> Vec<f32> {
        (0..len)
            .map(|index| {
                let value = u8::try_from((index * 37 + seed * 19) % 251).unwrap();
                (f32::from(value) - 125.0) / 1_250.0
            })
            .collect()
    }

    fn selected_matrix(matrix: &[f32], count: usize) -> (Vec<usize>, Vec<f32>) {
        let mut indices = (0..count)
            .map(|index| index * 37 % CONCEPTS)
            .collect::<Vec<_>>();
        indices.sort_unstable();
        let selected = indices
            .iter()
            .flat_map(|&index| matrix[index * DIM..(index + 1) * DIM].iter().copied())
            .collect();
        (indices, selected)
    }

    #[test]
    fn selected_columns_match_full_matrix_bit_for_bit() {
        let matrix = values(CONCEPTS * DIM, 17);
        for rows in [1, 7, 31, 255, 256, 257] {
            let embeddings = values(rows * DIM, 23);
            let full = dot_products(&embeddings, rows, &matrix, CONCEPTS, DIM);
            for count in [1, 3, 7, 23, 28, 31, 43] {
                let (indices, selected) = selected_matrix(&matrix, count);
                let compact = dot_products(&embeddings, rows, &selected, count, DIM);
                for row in 0..rows {
                    for (position, &index) in indices.iter().enumerate() {
                        assert_eq!(
                            full[row * CONCEPTS + index].to_bits(),
                            compact[row * count + position].to_bits(),
                            "row {row}, Concept {index}, compact position {position}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "synthetic release-profile matrix attribution"]
    fn selected_columns_matrix_profile() {
        use std::{hint::black_box, time::Instant};

        let rows = 256;
        let embeddings = values(rows * DIM, 23);
        let matrix = values(CONCEPTS * DIM, 17);
        let (indices, selected) = selected_matrix(&matrix, 28);
        for round in 0..=7 {
            for (name, data, count) in [
                ("full", matrix.as_slice(), CONCEPTS),
                ("selected", selected.as_slice(), indices.len()),
            ] {
                let start = Instant::now();
                for _ in 0..25 {
                    black_box(dot_products(&embeddings, rows, data, count, DIM));
                }
                eprintln!(
                    "{name} round={round} ms={:.3}",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            }
        }
    }
}
