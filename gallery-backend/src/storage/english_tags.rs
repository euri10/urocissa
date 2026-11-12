//! One-time rewrite of User tags that spelled a built-in Concept by its former French label.
//!
//! Vocabulary 2026-11-12.1 made Concepts English-only. Earlier confirmations wrote the French
//! label as an ordinary User tag (`coucher de soleil`), which would otherwise become an unrelated
//! personal Concept. This table exists only to translate those stored tags; delete it with the
//! migration once every deployed library carries the marker.
use std::collections::HashSet;

use anyhow::Result;
use redb::{Durability, ReadableDatabase, TableDefinition};

use super::DataStore;
use crate::{
    operations::autotag::normalize_name, public::structure::abstract_data::AbstractData,
    public::structure::object::next_mutation_timestamp,
};

const TAG_MIGRATIONS: TableDefinition<&str, u64> = TableDefinition::new("tag_migrations");
const MIGRATION: &str = "english_concept_tags_v1";

/// Former French labels and synonyms (normalized) and the English label that replaces them.
/// Words spelled the same in both languages (`portrait`, `animal`) are absent: nothing changes.
const LEGACY_FRENCH: &[(&str, &str)] = &[
    ("abeille", "bee"),
    ("anniversaire", "birthday party"),
    ("arbre", "tree"),
    ("arc-en-ciel", "rainbow"),
    ("avion", "airplane"),
    ("banderole", "banner"),
    ("bateau", "boat"),
    ("bicyclette", "bicycle"),
    ("bière", "beer"),
    ("bâtiment", "building"),
    ("bébé", "baby"),
    ("café", "coffee"),
    ("canard", "duck"),
    ("cascade", "waterfall"),
    ("cerf", "deer"),
    ("chat", "cat"),
    ("cheval", "horse"),
    ("chien", "dog"),
    ("château", "castle"),
    ("cigogne", "stork"),
    ("coucher de soleil", "sunset"),
    ("cyclisme", "cycling"),
    ("cygne", "swan"),
    ("drapeau", "flag"),
    ("enfant", "child"),
    ("feu de camp", "campfire"),
    ("feu d’artifice", "fireworks"),
    ("fleur", "flower"),
    ("forêt", "forest"),
    ("foule", "crowd"),
    ("guitare", "guitar"),
    ("gâteau", "cake"),
    ("jardin", "garden"),
    ("lac", "lake"),
    ("livre", "book"),
    ("maison", "house"),
    ("manifestation", "protest"),
    ("mariage", "wedding"),
    ("mer", "sea"),
    ("montagne", "mountain"),
    ("mouton", "sheep"),
    ("natation", "swimming"),
    ("neige", "snow"),
    ("nourriture", "food"),
    ("nuage", "cloud"),
    ("oiseau", "bird"),
    ("ordinateur", "computer"),
    ("pain", "bread"),
    ("papillon", "butterfly"),
    ("parc", "park"),
    ("personne", "person"),
    ("plage", "beach"),
    ("pont", "bridge"),
    ("randonnée", "hiking"),
    ("rivière", "river"),
    ("rue", "street"),
    ("ski", "skiing"),
    ("tente", "tent"),
    ("vache", "cow"),
    ("vin", "wine"),
    ("voiture", "car"),
    ("vélo", "bicycle"),
];

fn english(tag: &str) -> Option<&'static str> {
    let name = normalize_name(tag);
    LEGACY_FRENCH
        .iter()
        .find_map(|&(french, english)| (french == name).then_some(english))
}

/// The tag set with every former French Concept label replaced, or `None` when unchanged.
pub(crate) fn english_tags(tags: &HashSet<String>) -> Option<HashSet<String>> {
    if !tags.iter().any(|tag| english(tag).is_some()) {
        return None;
    }
    Some(
        tags.iter()
            .map(|tag| english(tag).map_or_else(|| tag.clone(), str::to_owned))
            .collect(),
    )
}

impl DataStore {
    /// Run before ingestion or HTTP startup. Idempotent: the rewrite is a fixed point, and the
    /// marker only skips the scan on later starts. Returns the number of rewritten records.
    pub(crate) fn migrate_english_concept_tags(&self) -> Result<usize> {
        {
            let txn = self.db.begin_read()?;
            if let Ok(meta) = txn.open_table(TAG_MIGRATIONS)
                && meta.get(MIGRATION)?.is_some()
            {
                return Ok(0);
            }
        }
        let changed = self.read(|reader| {
            let mut changed = Vec::new();
            for entry in reader.iter()? {
                let (_, value) = entry?;
                let mut record: AbstractData = value.into_value();
                if let Some(tags) = english_tags(record.tag()) {
                    *record.tag_mut() = tags;
                    changed.push(record);
                }
            }
            Ok::<_, anyhow::Error>(changed)
        })?;
        let count = changed.len();
        if count > 0 {
            let changed_at = next_mutation_timestamp();
            self.write(|writer| {
                for mut record in changed {
                    record.touch_update_at(changed_at);
                    writer.insert(&record)?;
                }
                Ok::<_, anyhow::Error>(())
            })?;
            info!("Rewrote former French Concept tags as English on {count} records");
        }
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        txn.open_table(TAG_MIGRATIONS)?.insert(MIGRATION, 1)?;
        txn.commit()?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
