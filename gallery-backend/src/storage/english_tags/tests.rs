use std::collections::HashSet;

use arrayvec::ArrayString;

use super::*;
use crate::{
    operations::autotag::BuiltinCatalog,
    public::structure::{
        album::Album,
        image::{ImageCombined, ImageMetadata},
        object::{ObjectSchema, ObjectType},
    },
};

fn set(tags: &[&str]) -> HashSet<String> {
    tags.iter().map(|&t| t.to_owned()).collect()
}

fn photo(id: &str, tags: &[&str]) -> AbstractData {
    let id = ArrayString::from(id).unwrap();
    let mut record = AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata: ImageMetadata::new(id, 100, 32, 24, "jpg".into()),
    });
    *record.tag_mut() = set(tags);
    record
}

#[test]
fn every_replacement_names_a_current_concept_and_no_french_name_remains() {
    let vocabulary = BuiltinCatalog::embedded()
        .unwrap()
        .alias_vocabulary()
        .unwrap();
    for &(french, english) in LEGACY_FRENCH {
        assert_eq!(normalize_name(french), french);
        assert!(
            vocabulary.concept_named(french).is_none(),
            "{french} still resolves"
        );
        assert!(
            vocabulary.concept_named(english).is_some(),
            "{english} is not a Concept"
        );
    }
}

#[test]
fn rewrites_only_former_french_concept_labels() {
    assert_eq!(
        english_tags(&set(&["sunset", "portrait", "Lyon 2024"])),
        None
    );
    assert_eq!(
        english_tags(&set(&[
            " Coucher de Soleil ",
            "nourriture",
            "Lyon 2024",
            "portrait"
        ])),
        Some(set(&["sunset", "food", "Lyon 2024", "portrait"]))
    );
    // Both spellings on one photo collapse into one English tag.
    assert_eq!(
        english_tags(&set(&["coucher de soleil", "sunset"])),
        Some(set(&["sunset"]))
    );
    // Near misses are personal tags, not former Concept labels.
    assert_eq!(english_tags(&set(&["couchers de soleil", "chats"])), None);
}

#[test]
fn migration_rewrites_records_once_and_marks_completion() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    let mut album = Album::new(ArrayString::from("album").unwrap(), None).into_abstract_data();
    *album.tag_mut() = set(&["mariage"]);
    store.write(|writer| {
        writer.insert(&photo("french", &["coucher de soleil", "Lyon"]))?;
        writer.insert(&photo("english", &["sunset"]))?;
        writer.insert(&album)?;
        Ok::<_, anyhow::Error>(())
    })?;

    assert_eq!(store.migrate_english_concept_tags()?, 2);
    let tags = |id: &str| -> Result<HashSet<String>> {
        Ok(store.reader()?.get(id)?.unwrap().into_value().tag().clone())
    };
    assert_eq!(tags("french")?, set(&["sunset", "Lyon"]));
    assert_eq!(tags("english")?, set(&["sunset"]));
    assert_eq!(tags("album")?, set(&["wedding"]));

    // The marker skips later scans, even if an old binary wrote a French tag meanwhile.
    store.write(|writer| writer.insert(&photo("late", &["chat"])))?;
    assert_eq!(store.migrate_english_concept_tags()?, 0);
    assert_eq!(tags("late")?, set(&["chat"]));
    Ok(())
}
