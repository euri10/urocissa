//! Concepts, their validated vocabulary, name matching and the parent hierarchy.
use std::collections::HashMap;

use super::error::AutoTagError;
use super::scoring::check_vector;

/// A vocabulary entry for auto-tagging. Built-in Concepts come from the versioned vocabulary
/// file; personal Concepts are derived from User tags.
#[derive(Debug, Clone, PartialEq)]
pub struct Concept {
    /// Stable ID; also the deterministic tie-break between equal confidences.
    pub id: String,
    /// The English name: the scoring prompt label, and the owner-facing label unless a User tag
    /// aliases the Concept library-wide.
    pub label: String,
    /// Further names a User tag may use to alias this Concept.
    pub synonyms: Vec<String>,
    /// Parent Concept IDs. The hierarchy must be acyclic.
    pub parents: Vec<String>,
    /// The calibrated zero-shot probability at which the Concept fires.
    pub threshold: f32,
    /// The L2-normalized text embedding of the English prompt, from the same model as the
    /// photo embeddings.
    pub vector: Vec<f32>,
}

/// The normalized form used to match User tags against Concept labels and synonyms: trimmed and
/// Unicode-lowercased, compared exactly.
pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A validated set of Concepts: unique IDs, unambiguous names, valid thresholds, vectors of one
/// dimension, existing parents and an acyclic hierarchy.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    dim: usize,
    /// Sorted by ID, so index order is the tie-break order.
    concepts: Vec<Concept>,
    /// The Concept vectors packed row-major for batched scoring.
    matrix: Vec<f32>,
    /// The transitive ancestors of each Concept, as sorted indices.
    ancestors: Vec<Vec<usize>>,
    by_id: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

impl Vocabulary {
    pub fn new(dim: usize, mut concepts: Vec<Concept>) -> Result<Self, AutoTagError> {
        if dim == 0 {
            return Err(AutoTagError::ZeroDimension);
        }
        concepts.sort_by(|left, right| left.id.cmp(&right.id));
        let mut by_id: HashMap<String, usize> = HashMap::with_capacity(concepts.len());
        let mut by_name: HashMap<String, usize> = HashMap::with_capacity(concepts.len() * 2);
        let mut matrix = Vec::with_capacity(concepts.len() * dim);
        for (index, concept) in concepts.iter().enumerate() {
            validate_concept(concept, dim)?;
            if by_id.insert(concept.id.clone(), index).is_some() {
                return Err(AutoTagError::DuplicateConcept {
                    concept: concept.id.clone(),
                });
            }
            let names = std::iter::once(&concept.label).chain(&concept.synonyms);
            for name in names {
                let name = normalize_name(name);
                if let Some(&other) = by_name.get(&name) {
                    if other != index {
                        return Err(AutoTagError::AmbiguousName {
                            name,
                            first: concepts[other].id.clone(),
                            second: concept.id.clone(),
                        });
                    }
                } else {
                    by_name.insert(name, index);
                }
            }
            matrix.extend_from_slice(&concept.vector);
        }
        let ancestors = ancestor_closure(&concepts, &by_id)?;
        Ok(Self {
            dim,
            concepts,
            matrix,
            ancestors,
            by_id,
            by_name,
        })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// All Concepts, sorted by ID.
    pub fn concepts(&self) -> &[Concept] {
        &self.concepts
    }

    pub fn concept(&self, id: &str) -> Option<&Concept> {
        self.index_of(id).map(|index| &self.concepts[index])
    }

    /// The Concept a User tag (or a `tag:` search term) names, by normalized label or
    /// synonym.
    pub fn concept_named(&self, name: &str) -> Option<&Concept> {
        self.index_named(name).map(|index| &self.concepts[index])
    }

    /// Every transitive ancestor of a Concept, sorted by ID; empty for an unknown ID.
    pub fn ancestors(&self, id: &str) -> impl Iterator<Item = &Concept> {
        self.index_of(id)
            .map_or(&[][..], |index| &self.ancestors[index])
            .iter()
            .map(|&ancestor| &self.concepts[ancestor])
    }

    pub(super) fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    pub(super) fn index_named(&self, name: &str) -> Option<usize> {
        self.by_name.get(&normalize_name(name)).copied()
    }

    pub(super) fn ancestor_indices(&self, index: usize) -> &[usize] {
        &self.ancestors[index]
    }

    pub(super) fn matrix(&self) -> &[f32] {
        &self.matrix
    }
}

fn validate_concept(concept: &Concept, dim: usize) -> Result<(), AutoTagError> {
    let names = [&concept.id, &concept.label]
        .into_iter()
        .chain(&concept.synonyms);
    if names.into_iter().any(|name| name.trim().is_empty()) {
        return Err(AutoTagError::EmptyName {
            concept: concept.id.clone(),
        });
    }
    let threshold = concept.threshold;
    if !(threshold > 0.0 && threshold < 1.0) {
        return Err(AutoTagError::InvalidThreshold {
            concept: concept.id.clone(),
            threshold,
        });
    }
    check_vector(&concept.vector, dim).map_err(|problem| AutoTagError::InvalidConceptVector {
        concept: concept.id.clone(),
        problem,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Active,
    Done,
}

fn ancestor_closure(
    concepts: &[Concept],
    by_id: &HashMap<String, usize>,
) -> Result<Vec<Vec<usize>>, AutoTagError> {
    let mut parents = Vec::with_capacity(concepts.len());
    for concept in concepts {
        let mut resolved = Vec::with_capacity(concept.parents.len());
        for parent in &concept.parents {
            let Some(&index) = by_id.get(parent) else {
                return Err(AutoTagError::UnknownParent {
                    concept: concept.id.clone(),
                    parent: parent.clone(),
                });
            };
            resolved.push(index);
        }
        parents.push(resolved);
    }
    let mut visits = vec![Visit::New; concepts.len()];
    let mut ancestors = vec![Vec::new(); concepts.len()];
    for index in 0..concepts.len() {
        visit(index, concepts, &parents, &mut visits, &mut ancestors)?;
    }
    Ok(ancestors)
}

fn visit(
    index: usize,
    concepts: &[Concept],
    parents: &[Vec<usize>],
    visits: &mut [Visit],
    ancestors: &mut [Vec<usize>],
) -> Result<(), AutoTagError> {
    match visits[index] {
        Visit::Done => return Ok(()),
        Visit::Active => {
            return Err(AutoTagError::HierarchyCycle {
                concept: concepts[index].id.clone(),
            });
        }
        Visit::New => visits[index] = Visit::Active,
    }
    let mut closure = Vec::new();
    for &parent in &parents[index] {
        visit(parent, concepts, parents, visits, ancestors)?;
        closure.push(parent);
        closure.extend_from_slice(&ancestors[parent]);
    }
    closure.sort_unstable();
    closure.dedup();
    ancestors[index] = closure;
    visits[index] = Visit::Done;
    Ok(())
}
