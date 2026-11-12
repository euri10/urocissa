//! Library-wide aliases: which User tag text a Concept displays instead of its label.
use std::collections::HashMap;

use super::vocabulary::Vocabulary;

/// For each Concept named by at least one User tag in the library, the User tag text it
/// displays. Per-photo aliasing (no Auto tag where the photo carries the User tag) needs only
/// the [`Vocabulary`]; this index decides the label everywhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AliasIndex {
    labels: HashMap<String, String>,
}

impl AliasIndex {
    /// Builds the index from the library's User tags and their photo counts. When several User
    /// tags name one Concept, the most used wins, then the lexicographically smallest text.
    /// Tags used on no photo (count 0) alias nothing.
    pub fn build<'a>(
        vocabulary: &Vocabulary,
        library_user_tags: impl IntoIterator<Item = (&'a str, usize)>,
    ) -> Self {
        let mut chosen: HashMap<usize, (&'a str, usize)> = HashMap::new();
        for (tag, count) in library_user_tags {
            if count == 0 {
                continue;
            }
            let Some(index) = vocabulary.index_named(tag) else {
                continue;
            };
            chosen
                .entry(index)
                .and_modify(|best| {
                    if (count, std::cmp::Reverse(tag)) > (best.1, std::cmp::Reverse(best.0)) {
                        *best = (tag, count);
                    }
                })
                .or_insert((tag, count));
        }
        let concepts = vocabulary.concepts();
        Self {
            labels: chosen
                .into_iter()
                .map(|(index, (tag, _))| (concepts[index].id.clone(), tag.to_owned()))
                .collect(),
        }
    }

    /// The User tag text that Concept displays, if any User tag aliases it.
    pub fn alias(&self, concept_id: &str) -> Option<&str> {
        self.labels.get(concept_id).map(String::as_str)
    }
}
