use crate::operations::visibility::PhotoVisibility;
use crate::public::structure::{album::AlbumCombined, audience::Viewer, object::ObjectType};
use crate::router::AppResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

use super::Tree;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct FacetValueInfo {
    pub value: String,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SearchFacets {
    pub tags: Vec<FacetValueInfo>,
    pub makes: Vec<FacetValueInfo>,
    pub models: Vec<FacetValueInfo>,
}

impl Tree {
    /// The tag-edit response is owner-only and must retain the unified facet list.
    pub fn read_tags(&self) -> AppResult<Vec<FacetValueInfo>> {
        let start_time = Instant::now();
        let state = self.state.read().unwrap();
        let mut visible = super::state::DenseBitmap::default();
        for slot in state.order.iter() {
            visible.set(slot.index(), true);
        }
        let tags = crate::operations::tag_search::OwnerTagIndex::read(&state)?.facets(
            &visible,
            &state.query.trashed,
            None,
        );
        crate::perf_timing!(
            "get_list.read_tags",
            start_time,
            "Read {} tags from cache",
            tags.len()
        );
        Ok(tags)
    }

    pub fn read_search_facets(
        &self,
        visibility: &PhotoVisibility,
        trashed: Option<bool>,
    ) -> AppResult<SearchFacets> {
        let start_time = Instant::now();
        let state = self.state.read().unwrap();
        let mut visible = super::state::DenseBitmap::default();
        for &slot in state.order.iter() {
            // A share's gallery contains members, not the album container itself.
            if matches!(visibility.viewer, Viewer::Share(_))
                && state
                    .get(slot)
                    .is_some_and(|record| record.object_type == ObjectType::Album)
            {
                continue;
            }
            if visibility.allows_cached(&state, slot)? {
                visible.set(slot.index(), true);
            }
        }
        let facets = SearchFacets {
            tags: if visibility.viewer == Viewer::Owner {
                crate::operations::tag_search::OwnerTagIndex::read(&state)?.facets(
                    &visible,
                    &state.query.trashed,
                    trashed,
                )
            } else {
                exact_facet_values_for_scope(
                    state.query.tags.iter(),
                    &state.query.trashed,
                    trashed,
                    Some(&visible),
                )
            },
            makes: camera_facet_values_for_scope(
                state.query.makes.iter(),
                &state.query.trashed,
                trashed,
                Some(&visible),
            ),
            models: camera_facet_values_for_scope(
                state.query.models.iter(),
                &state.query.trashed,
                trashed,
                Some(&visible),
            ),
        };
        crate::perf_timing!(
            "get_list.read_search_facets",
            start_time,
            "Read {} tags, {} makes, and {} models from cache",
            facets.tags.len(),
            facets.makes.len(),
            facets.models.len()
        );
        Ok(facets)
    }

    pub fn read_albums(&self) -> Vec<AlbumCombined> {
        let start_time = Instant::now();
        let mut albums = self
            .state
            .read()
            .unwrap()
            .albums
            .values()
            .cloned()
            .collect::<Vec<_>>();
        albums.sort_unstable_by_key(|album| album.object.id);
        crate::perf_timing!(
            "get_list.read_albums",
            start_time,
            "Read {} albums from cache",
            albums.len()
        );
        albums
    }

    pub fn with_state_update<R>(&self, operation: impl FnOnce() -> R) -> R {
        let _update_guard = self.state_update_lock.lock().unwrap();
        operation()
    }

    pub fn replace_tree_state(&self, state: super::state::TreeState) {
        *self.state.write().unwrap() = state;
    }
}

#[cfg(test)]
fn exact_facet_values<'a>(
    values: impl Iterator<Item = (&'a String, &'a super::state::OrdinalSet)>,
) -> Vec<FacetValueInfo> {
    exact_facet_values_for_scope(values, &super::state::DenseBitmap::default(), None, None)
}

fn exact_facet_values_for_scope<'a>(
    values: impl Iterator<Item = (&'a String, &'a super::state::OrdinalSet)>,
    trashed_members: &super::state::DenseBitmap,
    trashed: Option<bool>,
    visible: Option<&super::state::DenseBitmap>,
) -> Vec<FacetValueInfo> {
    let mut facets = values
        .filter_map(|(value, members)| {
            let count = count_members_in_scope(members, trashed_members, trashed, visible);
            (count > 0).then(|| FacetValueInfo {
                value: value.clone(),
                count,
                ..Default::default()
            })
        })
        .collect::<Vec<_>>();
    facets.sort_unstable_by(|left, right| left.value.cmp(&right.value));
    facets
}

#[cfg(test)]
fn camera_facet_values<'a>(
    values: impl Iterator<Item = (&'a String, &'a super::state::OrdinalSet)>,
) -> Vec<FacetValueInfo> {
    camera_facet_values_for_scope(values, &super::state::DenseBitmap::default(), None, None)
}

fn camera_facet_values_for_scope<'a>(
    values: impl Iterator<Item = (&'a String, &'a super::state::OrdinalSet)>,
    trashed_members: &super::state::DenseBitmap,
    trashed: Option<bool>,
    visible: Option<&super::state::DenseBitmap>,
) -> Vec<FacetValueInfo> {
    #[derive(Default)]
    struct Aggregate {
        count: usize,
        spellings: HashMap<String, usize>,
    }

    let mut aggregates = HashMap::<String, Aggregate>::new();
    for (raw_value, members) in values {
        let Some(value) = normalize_camera_facet_value(raw_value) else {
            continue;
        };
        let member_count = count_members_in_scope(members, trashed_members, trashed, visible);
        if member_count == 0 {
            continue;
        }
        let aggregate = aggregates.entry(value.to_ascii_lowercase()).or_default();
        aggregate.count = aggregate.count.saturating_add(member_count);
        let spelling_count = aggregate.spellings.entry(value).or_default();
        *spelling_count = spelling_count.saturating_add(member_count);
    }

    let mut facets = aggregates
        .into_iter()
        .filter_map(|(_, aggregate)| {
            let value = aggregate
                .spellings
                .into_iter()
                // Prefer the most common spelling, then the lexicographically
                // smallest spelling so HashMap iteration cannot affect output.
                .max_by(|(left_value, left_count), (right_value, right_count)| {
                    left_count
                        .cmp(right_count)
                        .then_with(|| right_value.cmp(left_value))
                })?
                .0;
            Some(FacetValueInfo {
                value,
                count: aggregate.count,
                ..Default::default()
            })
        })
        .collect::<Vec<_>>();
    facets.sort_unstable_by(|left, right| left.value.cmp(&right.value));
    facets
}

fn count_members_in_scope(
    members: &super::state::OrdinalSet,
    trashed_members: &super::state::DenseBitmap,
    trashed: Option<bool>,
    visible: Option<&super::state::DenseBitmap>,
) -> usize {
    match (trashed, visible) {
        (None, None) => members.len(),
        _ => members
            .iter()
            .filter(|ordinal| visible.is_none_or(|visible| visible.contains(*ordinal)))
            .filter(|ordinal| {
                trashed.is_none_or(|expected| trashed_members.contains(*ordinal) == expected)
            })
            .count(),
    }
}

fn normalize_camera_facet_value(value: &str) -> Option<String> {
    let mut trimmed = value.trim();
    while let Some(without_empty) = trimmed.strip_suffix("\"\"") {
        let without_empty = without_empty.trim_end();
        let Some(without_comma) = without_empty.strip_suffix(',') else {
            break;
        };
        trimmed = without_comma.trim_end();
    }
    let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
    .trim();
    (!unquoted.is_empty()).then(|| unquoted.to_owned())
}

#[cfg(test)]
mod search_facet_tests {
    use super::{
        FacetValueInfo, SearchFacets, camera_facet_values, camera_facet_values_for_scope,
        exact_facet_values, exact_facet_values_for_scope, normalize_camera_facet_value,
    };
    use crate::public::db::tree::state::OrdinalSet;
    use std::collections::HashMap;

    #[test]
    fn normalizes_camera_values_without_touching_inner_content() {
        assert_eq!(
            normalize_camera_facet_value("  \" Canon EOS \"  ").as_deref(),
            Some("Canon EOS")
        );
        assert_eq!(
            normalize_camera_facet_value("  Nikon  ").as_deref(),
            Some("Nikon")
        );
        assert_eq!(normalize_camera_facet_value(" \"  \" "), None);
        assert_eq!(
            normalize_camera_facet_value("\"Sony").as_deref(),
            Some("\"Sony")
        );
        assert_eq!(
            normalize_camera_facet_value(" \"OPPO\", \"\",\"\",  \"\" ").as_deref(),
            Some("OPPO")
        );
        assert_eq!(normalize_camera_facet_value("\"\", \"\", \"\""), None);
    }

    #[test]
    fn merges_ascii_case_variants_and_uses_a_stable_representative() {
        let values = HashMap::from([
            (
                "  \"Canon\"  ".to_owned(),
                OrdinalSet::from_ordinals([0, 1], 16),
            ),
            ("CANON".to_owned(), OrdinalSet::from_ordinals([2], 16)),
            ("Canon".to_owned(), OrdinalSet::from_ordinals([3, 4, 5], 16)),
            ("nikon".to_owned(), OrdinalSet::from_ordinals([6], 16)),
            ("NIKON".to_owned(), OrdinalSet::from_ordinals([7], 16)),
            (" \" \" ".to_owned(), OrdinalSet::from_ordinals([8], 16)),
        ]);

        assert_eq!(
            camera_facet_values(values.iter()),
            vec![
                FacetValueInfo {
                    value: "Canon".to_owned(),
                    count: 6,
                    ..Default::default()
                },
                FacetValueInfo {
                    value: "NIKON".to_owned(),
                    count: 2,
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn regular_tags_remain_case_sensitive() {
        let values = HashMap::from([
            ("Family".to_owned(), OrdinalSet::from_ordinals([0], 16)),
            ("family".to_owned(), OrdinalSet::from_ordinals([1, 2], 16)),
        ]);

        assert_eq!(
            exact_facet_values(values.iter()),
            vec![
                FacetValueInfo {
                    value: "Family".to_owned(),
                    count: 1,
                    ..Default::default()
                },
                FacetValueInfo {
                    value: "family".to_owned(),
                    count: 2,
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn filters_exact_facets_by_trash_state_and_omits_empty_values() {
        let values = HashMap::from([
            ("shared".to_owned(), OrdinalSet::from_ordinals([0, 1], 16)),
            ("active-only".to_owned(), OrdinalSet::from_ordinals([2], 16)),
            (
                "trashed-only".to_owned(),
                OrdinalSet::from_ordinals([3], 16),
            ),
        ]);
        let mut trashed_members = super::super::state::DenseBitmap::default();
        trashed_members.set(1, true);
        trashed_members.set(3, true);

        assert_eq!(
            exact_facet_values_for_scope(values.iter(), &trashed_members, Some(false), None),
            vec![
                FacetValueInfo {
                    value: "active-only".to_owned(),
                    count: 1,
                    ..Default::default()
                },
                FacetValueInfo {
                    value: "shared".to_owned(),
                    count: 1,
                    ..Default::default()
                },
            ]
        );
        assert_eq!(
            exact_facet_values_for_scope(values.iter(), &trashed_members, Some(true), None),
            vec![
                FacetValueInfo {
                    value: "shared".to_owned(),
                    count: 1,
                    ..Default::default()
                },
                FacetValueInfo {
                    value: "trashed-only".to_owned(),
                    count: 1,
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn filters_camera_facets_before_case_variant_aggregation() {
        let values = HashMap::from([
            ("Canon".to_owned(), OrdinalSet::from_ordinals([0, 1], 16)),
            ("CANON".to_owned(), OrdinalSet::from_ordinals([2], 16)),
            ("Nikon".to_owned(), OrdinalSet::from_ordinals([3], 16)),
        ]);
        let mut trashed_members = super::super::state::DenseBitmap::default();
        trashed_members.set(1, true);
        trashed_members.set(2, true);
        trashed_members.set(3, true);

        assert_eq!(
            camera_facet_values_for_scope(values.iter(), &trashed_members, Some(false), None),
            vec![FacetValueInfo {
                value: "Canon".to_owned(),
                count: 1,
                ..Default::default()
            }]
        );
        assert_eq!(
            camera_facet_values_for_scope(values.iter(), &trashed_members, Some(true), None),
            vec![
                FacetValueInfo {
                    value: "CANON".to_owned(),
                    count: 2,
                    ..Default::default()
                },
                FacetValueInfo {
                    value: "Nikon".to_owned(),
                    count: 1,
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn serializes_the_unified_search_facet_contract() {
        let facets = SearchFacets {
            tags: vec![FacetValueInfo {
                value: "family".to_owned(),
                count: 2,
                ..Default::default()
            }],
            makes: vec![FacetValueInfo {
                value: "Canon".to_owned(),
                count: 4,
                ..Default::default()
            }],
            models: vec![],
        };

        assert_eq!(
            serde_json::to_value(facets).unwrap(),
            serde_json::json!({
                "tags": [{ "value": "family", "count": 2 }],
                "makes": [{ "value": "Canon", "count": 4 }],
                "models": []
            })
        );
    }
}
