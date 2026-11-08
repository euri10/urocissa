//! The pure Auto tag engine: photo embeddings, Concepts and owner feedback in, materialized
//! Auto tags out. No HTTP, storage, model or UI; callers supply vectors and feedback.
//!
//! # Rules, in order of precedence
//!
//! 1. **Auto-tags-off**: a photo with the flag gets no Auto tags at all.
//! 2. **Rejection always wins**: a rejected Concept never appears on that photo, displayed or
//!    hidden, fired or implied, whatever its score or classifier, and even when one of the
//!    photo's User tags names it. A Rejection removes only that Concept: its ancestors may
//!    still be implied by other present Concepts.
//! 3. **Alias**: a User tag whose trimmed, Unicode-lowercased text equals a Concept's French
//!    label, English label or synonym aliases that Concept. A photo carrying the User tag gets
//!    no Auto tag for the Concept (the User tag is what shows). Library-wide, an aliased
//!    Concept displays the User tag text instead of its French label ([`AliasIndex`]).
//! 4. **Scoring**: zero-shot probability `sigmoid(logit_scale * dot + logit_bias)`; a Concept
//!    fires when the probability reaches its threshold. A [`ConceptClassifier`] may replace the
//!    probability and threshold per photo, or make a Concept [`ConceptMode::Inactive`] (it then
//!    never fires; hierarchy and aliases still apply to it).
//! 5. **Hierarchy**: Concepts *present* on the photo are the fired ones plus those aliased by
//!    its User tags. Every transitive ancestor of a present Concept is recorded but never
//!    displayed ([`Placement::SuppressedAncestor`] if it fired itself, otherwise
//!    [`Placement::Implied`]), so `tag:oiseau` still finds storks. Its confidence is the
//!    maximum of its own fired probability and its best present descendant's (an aliasing User
//!    tag counts as 1). Ancestors present as aliasing User tags get no Auto tag.
//! 6. **Cap**: the remaining (specific) fired Concepts are ranked by confidence, then Concept
//!    ID; the first [`MAX_DISPLAYED`] are [`Placement::Displayed`], the rest
//!    [`Placement::OverCap`].
//!
//! # Output
//!
//! Each photo yields its Auto tags sorted by confidence descending, then Concept ID. Every tag
//! matches owner tag search (including parent expansion); only [`Placement::Displayed`] tags are
//! chips. A detail view may list [`Placement::OverCap`] tags as further specific tags; ancestor
//! placements exist for search and facets. Labels are the alias override or the French label.
//!
//! # Determinism and performance
//!
//! Identical inputs give identical outputs, independent of thread count and batch composition:
//! [`AutoTagEngine::tag_photo`] runs the same code as [`AutoTagEngine::tag_library`] and the
//! matrix multiplication accumulates each dot product in a fixed order. A full recompute is one
//! `sgemm` per 256-photo chunk (runtime-selected AVX2/AVX-512 kernels) spread over rayon, then a
//! linear threshold pass that evaluates the sigmoid only near each threshold.
//! Measured on the dev box (Ryzen AI 7 PRO 350, release, `full_recompute_benchmark`): 10k photos
//! × 2k Concepts × 768 dimensions in about 95 ms on 16 threads, 250 ms on 2 and 480 ms on 1
//! (`RAYON_NUM_THREADS`); one photo in about 0.8 ms. The synthetic hierarchy is deliberately
//! deep (about 25 fired and 52 implied Auto tags per photo); the matrix multiplication is about
//! two thirds of the single-thread time.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        unused_imports,
        reason = "consumed by Auto tag storage and materialization (infra-urocissa-autotag-sjfz.5)"
    )
)]

mod alias;
mod builtin;
mod classifier;
mod engine;
mod error;
pub mod learning;
mod scoring;
mod vocabulary;

pub use alias::AliasIndex;
pub use builtin::{BuiltinCatalog, BuiltinConcept};
pub use classifier::{Classification, ConceptClassifier, ConceptMode, ZeroShot};
pub use engine::{AutoTag, AutoTagEngine, MAX_DISPLAYED, PhotoInput, PhotoResult, Placement};
pub use error::{AutoTagError, VectorProblem};
pub use scoring::{NORM_TOLERANCE, ScoreModel, check_vector};
pub use vocabulary::{Concept, Vocabulary, normalize_name};

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;
