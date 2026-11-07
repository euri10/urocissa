//! Materializes Auto tags for one photo or a whole library.
use rayon::prelude::*;

use super::alias::AliasIndex;
use super::classifier::{ConceptClassifier, ConceptMode, ZeroShot};
use super::error::AutoTagError;
use super::scoring::{ScoreModel, check_vector, dot_products};
use super::vocabulary::Vocabulary;

/// At most this many Auto tags per photo are displayed as chips.
pub const MAX_DISPLAYED: usize = 8;

/// Photos scored per matrix multiplication. Bounds the dot-product buffer to
/// `CHUNK × Concepts` floats per worker (2 MiB at 2k Concepts) while amortizing `sgemm`'s
/// packing of the Concept matrix; 256 beat 128 end to end at 1, 2 and 16 threads.
const CHUNK: usize = 256;

/// One photo's inputs. Borrowed so that a full recompute copies nothing up front. A photo
/// without an embedding (pending, or a video) is not an input and so has no Auto tags.
#[derive(Debug, Clone, Copy)]
pub struct PhotoInput<'a> {
    /// The L2-normalized embedding, from the model that produced the Concept vectors.
    pub embedding: &'a [f32],
    /// The photo's User tags, as authored.
    pub user_tags: &'a [&'a str],
    /// IDs of the Concepts the owner rejected on this photo. Unknown IDs are ignored.
    pub rejections: &'a [&'a str],
    /// The photo's auto-tags-off flag.
    pub auto_tags_off: bool,
}

/// Why an Auto tag is or is not shown as a chip. Every placement matches tag search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// A chip: one of the at most [`MAX_DISPLAYED`] most confident specific Auto tags.
    Displayed,
    /// Fired and specific, but ranked beyond the chip cap.
    OverCap,
    /// Fired on its own, but hidden because a descendant is present on the photo.
    SuppressedAncestor,
    /// Did not fire on its own; recorded because a descendant is present on the photo.
    Implied,
}

/// A materialized Auto tag, borrowing its strings from the engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoTag<'e> {
    pub concept: &'e str,
    /// The aliasing User tag text if the library has one, otherwise the French label.
    pub label: &'e str,
    /// The (classifier or zero-shot) probability for fired Concepts. For ancestors, the maximum
    /// of their own fired probability and their best present descendant's confidence, where an
    /// aliasing User tag counts as 1.
    pub confidence: f32,
    pub placement: Placement,
}

pub type PhotoResult<'e> = Result<Vec<AutoTag<'e>>, AutoTagError>;

/// The pure Auto tag engine: embeddings, Concepts and owner feedback in, Auto tags out.
pub struct AutoTagEngine {
    vocabulary: Vocabulary,
    model: ScoreModel,
    classifier: Box<dyn ConceptClassifier>,
    /// Display label per Concept index.
    labels: Vec<String>,
    /// Per Concept index, the zero-shot dot cut, or infinity when the Concept is not zero-shot.
    cuts: Vec<f32>,
    /// Concept indices decided per photo by the classifier.
    classified: Vec<usize>,
}

impl AutoTagEngine {
    /// A zero-shot engine that displays French labels.
    pub fn new(vocabulary: Vocabulary, model: ScoreModel) -> Self {
        let labels = vocabulary
            .concepts()
            .iter()
            .map(|concept| concept.label_fr.clone())
            .collect();
        let mut engine = Self {
            vocabulary,
            model,
            classifier: Box::new(ZeroShot),
            labels,
            cuts: Vec::new(),
            classified: Vec::new(),
        };
        engine.plan();
        engine
    }

    /// Displays each aliased Concept under its library-wide User tag instead of its French
    /// label. Replaces any previous aliases.
    #[must_use]
    pub fn with_aliases(mut self, aliases: &AliasIndex) -> Self {
        for (label, concept) in self.labels.iter_mut().zip(self.vocabulary.concepts()) {
            aliases
                .alias(&concept.id)
                .unwrap_or(&concept.label_fr)
                .clone_into(label);
        }
        self
    }

    /// Replaces the classifier hook; its [`ConceptClassifier::mode`] is read once, here.
    #[must_use]
    pub fn with_classifier(mut self, classifier: impl ConceptClassifier + 'static) -> Self {
        self.classifier = Box::new(classifier);
        self.plan();
        self
    }

    pub fn vocabulary(&self) -> &Vocabulary {
        &self.vocabulary
    }

    /// Auto tags for one photo (incremental updates). Runs the same code as
    /// [`Self::tag_library`], so the result is identical to that photo's library result.
    pub fn tag_photo(&self, photo: &PhotoInput<'_>) -> PhotoResult<'_> {
        self.tag_chunk(std::slice::from_ref(photo))
            .pop()
            .expect("one result per photo")
    }

    /// Auto tags for every photo, in input order, scored in parallel on the current rayon pool.
    /// An invalid embedding fails only its own photo.
    pub fn tag_library(&self, photos: &[PhotoInput<'_>]) -> Vec<PhotoResult<'_>> {
        photos
            .par_chunks(CHUNK)
            .flat_map_iter(|chunk| self.tag_chunk(chunk))
            .collect()
    }

    fn plan(&mut self) {
        self.cuts.clear();
        self.classified.clear();
        for (index, concept) in self.vocabulary.concepts().iter().enumerate() {
            let cut = match self.classifier.mode(concept) {
                ConceptMode::ZeroShot => self.model.dot_cut(concept.threshold),
                ConceptMode::Classifier => {
                    self.classified.push(index);
                    f32::INFINITY
                }
                ConceptMode::Inactive => f32::INFINITY,
            };
            self.cuts.push(cut);
        }
    }

    fn tag_chunk(&self, photos: &[PhotoInput<'_>]) -> Vec<PhotoResult<'_>> {
        let dim = self.vocabulary.dim();
        let concepts = self.cuts.len();
        let mut results: Vec<PhotoResult<'_>> = photos
            .iter()
            .map(|photo| {
                check_vector(photo.embedding, dim)
                    .map(|()| Vec::new())
                    .map_err(AutoTagError::InvalidEmbedding)
            })
            .collect();
        let scored: Vec<usize> = (0..photos.len())
            .filter(|&index| results[index].is_ok() && !photos[index].auto_tags_off)
            .collect();
        let mut block = Vec::with_capacity(scored.len() * dim);
        for &index in &scored {
            block.extend_from_slice(photos[index].embedding);
        }
        let dots = dot_products(
            &block,
            scored.len(),
            self.vocabulary.matrix(),
            concepts,
            dim,
        );
        for (row, &index) in scored.iter().enumerate() {
            let dots = &dots[row * concepts..(row + 1) * concepts];
            results[index] = Ok(self.resolve(&photos[index], dots));
        }
        results
    }

    /// Applies firing, Rejections, aliases, the hierarchy and the cap to one scored photo.
    fn resolve(&self, photo: &PhotoInput<'_>, dots: &[f32]) -> Vec<AutoTag<'_>> {
        let vocabulary = &self.vocabulary;
        let concepts = vocabulary.concepts();
        let mut rejected: Vec<usize> = photo
            .rejections
            .iter()
            .filter_map(|id| vocabulary.index_of(id))
            .collect();
        rejected.sort_unstable();
        rejected.dedup();
        // A Rejection wins over an aliasing User tag too: that Concept is absent here.
        let mut aliased: Vec<usize> = photo
            .user_tags
            .iter()
            .filter_map(|tag| vocabulary.index_named(tag))
            .filter(|index| rejected.binary_search(index).is_err())
            .collect();
        aliased.sort_unstable();
        aliased.dedup();
        let excluded = |index: usize| {
            rejected.binary_search(&index).is_ok() || aliased.binary_search(&index).is_ok()
        };

        let mut fired = Vec::new();
        for (index, (&dot, &cut)) in dots.iter().zip(&self.cuts).enumerate() {
            if dot >= cut && !excluded(index) {
                let probability = self.model.probability(dot);
                if probability >= concepts[index].threshold {
                    fired.push((index, probability));
                }
            }
        }
        for &index in &self.classified {
            if !excluded(index) {
                let zero_shot = self.model.probability(dots[index]);
                let decision =
                    self.classifier
                        .classify(&concepts[index], photo.embedding, zero_shot);
                if decision.probability >= decision.threshold {
                    fired.push((index, decision.probability));
                }
            }
        }
        fired.sort_unstable_by_key(|&(index, _)| index);

        // Every transitive ancestor of a present Concept, with its best descendant confidence.
        let present = fired
            .iter()
            .copied()
            .chain(aliased.iter().map(|&index| (index, 1.0)));
        let mut ancestors = Vec::new();
        for (index, confidence) in present {
            ancestors.extend(
                vocabulary
                    .ancestor_indices(index)
                    .iter()
                    .filter(|&&ancestor| !excluded(ancestor))
                    .map(|&ancestor| (ancestor, confidence)),
            );
        }
        ancestors.sort_unstable_by_key(|&(index, _)| index);
        ancestors.dedup_by(|next, kept| {
            let same = next.0 == kept.0;
            if same {
                kept.1 = kept.1.max(next.1);
            }
            same
        });

        let mut tags = Vec::with_capacity(fired.len() + ancestors.len());
        let mut specific = Vec::with_capacity(fired.len());
        for &(index, probability) in &fired {
            match ancestors.binary_search_by_key(&index, |&(ancestor, _)| ancestor) {
                Ok(position) => tags.push(self.tag(
                    index,
                    probability.max(ancestors[position].1),
                    Placement::SuppressedAncestor,
                )),
                Err(_) => specific.push((index, probability)),
            }
        }
        for &(index, confidence) in &ancestors {
            if fired.binary_search_by_key(&index, |&(f, _)| f).is_err() {
                tags.push(self.tag(index, confidence, Placement::Implied));
            }
        }
        specific
            .sort_unstable_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
        for (rank, &(index, probability)) in specific.iter().enumerate() {
            let placement = if rank < MAX_DISPLAYED {
                Placement::Displayed
            } else {
                Placement::OverCap
            };
            tags.push(self.tag(index, probability, placement));
        }
        tags.sort_unstable_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.concept.cmp(right.concept))
        });
        tags
    }

    fn tag(&self, index: usize, confidence: f32, placement: Placement) -> AutoTag<'_> {
        AutoTag {
            concept: &self.vocabulary.concepts()[index].id,
            label: &self.labels[index],
            confidence,
            placement,
        }
    }
}
