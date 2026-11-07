use std::fmt;

/// Why a vector (a Concept vector or a photo embedding) was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VectorProblem {
    DimensionMismatch {
        expected: usize,
        actual: usize,
    },
    NonFinite,
    /// The Euclidean norm is not within [`super::NORM_TOLERANCE`] of 1.
    NotNormalized {
        norm: f32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AutoTagError {
    ZeroDimension,
    InvalidScoreModel {
        logit_scale: f32,
        logit_bias: f32,
    },
    DuplicateConcept {
        concept: String,
    },
    /// An ID, label or synonym is empty after trimming.
    EmptyName {
        concept: String,
    },
    /// Two Concepts share a normalized label or synonym, so a User tag with that text could
    /// not alias exactly one Concept.
    AmbiguousName {
        name: String,
        first: String,
        second: String,
    },
    /// Thresholds are probabilities strictly between 0 and 1.
    InvalidThreshold {
        concept: String,
        threshold: f32,
    },
    UnknownParent {
        concept: String,
        parent: String,
    },
    HierarchyCycle {
        concept: String,
    },
    InvalidConceptVector {
        concept: String,
        problem: VectorProblem,
    },
    InvalidEmbedding(VectorProblem),
}

impl fmt::Display for VectorProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch { expected, actual } => {
                write!(f, "expected {expected} dimensions, got {actual}")
            }
            Self::NonFinite => f.write_str("contains a non-finite value"),
            Self::NotNormalized { norm } => write!(f, "is not L2-normalized (norm {norm})"),
        }
    }
}

impl fmt::Display for AutoTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => f.write_str("embedding dimension must be positive"),
            Self::InvalidScoreModel {
                logit_scale,
                logit_bias,
            } => write!(
                f,
                "invalid score model (logit scale {logit_scale}, logit bias {logit_bias})"
            ),
            Self::DuplicateConcept { concept } => write!(f, "duplicate Concept `{concept}`"),
            Self::EmptyName { concept } => {
                write!(f, "Concept `{concept}` has an empty ID, label or synonym")
            }
            Self::AmbiguousName {
                name,
                first,
                second,
            } => write!(
                f,
                "name `{name}` is shared by Concepts `{first}` and `{second}`"
            ),
            Self::InvalidThreshold { concept, threshold } => write!(
                f,
                "Concept `{concept}` threshold {threshold} is not strictly between 0 and 1"
            ),
            Self::UnknownParent { concept, parent } => {
                write!(f, "Concept `{concept}` has unknown parent `{parent}`")
            }
            Self::HierarchyCycle { concept } => {
                write!(f, "Concept hierarchy has a cycle through `{concept}`")
            }
            Self::InvalidConceptVector { concept, problem } => {
                write!(f, "Concept `{concept}` vector {problem}")
            }
            Self::InvalidEmbedding(problem) => write!(f, "photo embedding {problem}"),
        }
    }
}

impl std::error::Error for AutoTagError {}
