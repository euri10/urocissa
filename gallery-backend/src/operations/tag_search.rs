//! Request-local owner tag membership. Public/share paths keep their authored indexes.
use std::collections::HashMap;

use anyhow::Result;

use super::autotag::{AliasIndex, BuiltinCatalog, Vocabulary};
use crate::public::{
    db::tree::{
        TREE,
        read_tags::FacetValueInfo,
        state::{DenseBitmap, OrdinalSet, TreeState},
    },
    structure::{
        expression::{FilterValue, TagSource},
        object::ObjectType,
    },
};

#[derive(Default)]
struct Members {
    value: String,
    user: OrdinalSet,
    auto: OrdinalSet,
}

pub struct OwnerTagIndex {
    vocabulary: Vocabulary,
    members: HashMap<String, Members>,
    universe: usize,
}

impl OwnerTagIndex {
    pub fn read(state: &TreeState) -> Result<Self> {
        // Hierarchy/name validation is shared with the engine; no model or inference needed.
        let vocabulary = BuiltinCatalog::embedded()?.alias_vocabulary()?;
        let aliases = AliasIndex::build(
            &vocabulary,
            state.query.tags.iter().map(|(tag, members)| {
                let photos = members
                    .iter()
                    .filter(|&ordinal| {
                        state
                            .slot_for_ordinal(ordinal)
                            .and_then(|slot| state.get(slot))
                            .is_some_and(|r| r.object_type == ObjectType::Image)
                    })
                    .count();
                (tag.as_str(), photos)
            }),
        );
        let mut index = Self {
            vocabulary,
            members: HashMap::new(),
            universe: state.arena.capacity(),
        };
        let mut user_aliases = HashMap::<String, OrdinalSet>::new();
        for (tag, members) in state.query.tags.iter() {
            if let Some(concept) = index.vocabulary.concept_named(tag) {
                user_aliases
                    .entry(concept.id.clone())
                    .or_default()
                    .union_with(members, index.universe, |_| {});
                let ids = std::iter::once(concept)
                    .chain(index.vocabulary.ancestors(&concept.id))
                    .map(|c| {
                        (
                            c.id.clone(),
                            aliases.alias(&c.id).unwrap_or(&c.label_fr).to_owned(),
                        )
                    })
                    .collect::<Vec<_>>();
                for (id, label) in ids {
                    index.add(format!("concept:{id}"), label, members, TagSource::User);
                }
            } else {
                index.add(format!("tag:{tag}"), tag.clone(), members, TagSource::User);
            }
        }
        if let Some(tags) = super::embedding::service().and_then(|s| s.auto_tags.get()) {
            let settings = TREE.store.auto_tag_settings()?;
            for (id, row) in tags.search_rows()? {
                let Some(slot) = state.find(&id) else {
                    continue;
                };
                let record = state.get(slot).unwrap();
                if record.object_type != ObjectType::Image
                    || record.cache_version != row.thumbnail_version
                    || !settings.enabled(&id)?
                {
                    continue;
                }
                let member = OrdinalSet::from_ordinals([slot.index()], index.universe);
                for tag in row.tags {
                    // Authored aliases suppress the exact Auto Concept even if the
                    // sidecar is offline and its cached row predates confirmation.
                    if user_aliases
                        .get(&tag.concept)
                        .is_some_and(|members| members.contains(slot.index()))
                    {
                        continue;
                    }
                    // Materialization already includes implied/suppressed ancestors and over-cap tags.
                    if let Some(concept) = index.vocabulary.concept(&tag.concept) {
                        let label = aliases
                            .alias(&concept.id)
                            .unwrap_or(&concept.label_fr)
                            .to_owned();
                        index.add(
                            format!("concept:{}", tag.concept),
                            label,
                            &member,
                            TagSource::Auto,
                        );
                    } else {
                        // Personal Concepts use their authored tag label as their search name.
                        index.add(
                            format!("tag:{}", tag.label),
                            tag.label,
                            &member,
                            TagSource::Auto,
                        );
                    }
                }
            }
        }
        Ok(index)
    }

    fn add(&mut self, key: String, value: String, members: &OrdinalSet, source: TagSource) {
        let entry = self.members.entry(key).or_default();
        entry.value = value;
        let target = match source {
            TagSource::User => &mut entry.user,
            TagSource::Auto => &mut entry.auto,
        };
        target.union_with(members, self.universe, |_| {});
    }

    pub fn matching(&self, value: &FilterValue, source: Option<TagSource>) -> OrdinalSet {
        let key = match value {
            FilterValue::Value(name) => Some(
                self.vocabulary
                    .concept_named(name)
                    .map_or_else(|| format!("tag:{name}"), |c| format!("concept:{}", c.id)),
            ),
            FilterValue::Exists(_) => None,
        };
        let mut result = OrdinalSet::default();
        for (_, entry) in self
            .members
            .iter()
            .filter(|(k, _)| key.as_ref().is_none_or(|key| key == *k))
        {
            if source != Some(TagSource::Auto) {
                result.union_with(&entry.user, self.universe, |_| {});
            }
            if source != Some(TagSource::User) {
                result.union_with(&entry.auto, self.universe, |_| {});
            }
        }
        result
    }

    pub fn facets(
        &self,
        visible: &DenseBitmap,
        trashed: &DenseBitmap,
        trash: Option<bool>,
    ) -> Vec<FacetValueInfo> {
        let in_scope = |ordinal| {
            visible.contains(ordinal)
                && trash.is_none_or(|value| trashed.contains(ordinal) == value)
        };
        let mut facets = self
            .members
            .values()
            .filter_map(|entry| {
                let mut union = entry.user.clone();
                union.union_with(&entry.auto, self.universe, |_| {});
                let count = union.iter().filter(|&o| in_scope(o)).count();
                (count > 0).then(|| FacetValueInfo {
                    value: entry.value.clone(),
                    count,
                    user_count: Some(entry.user.iter().filter(|&o| in_scope(o)).count()),
                    auto_count: Some(entry.auto.iter().filter(|&o| in_scope(o)).count()),
                })
            })
            .collect::<Vec<_>>();
        facets.sort_unstable_by(|a, b| a.value.cmp(&b.value));
        facets
    }
}
