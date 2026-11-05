use arrayvec::ArrayString;
use redb::Database;

use super::*;
use crate::public::structure::{
    abstract_data::AbstractData,
    album::Album,
    image::{ImageCombined, ImageMetadata},
    object::{ObjectSchema, ObjectType},
    video::{VideoCombined, VideoMetadata},
};
use crate::storage::v6::V6AbstractData;

mod albums;

fn photo(id: &str) -> AbstractData {
    let id = ArrayString::from(id).unwrap();
    AbstractData::Image(ImageCombined {
        object: ObjectSchema::new(id, ObjectType::Image),
        metadata: ImageMetadata::new(id, 100, 32, 24, "jpg".into()),
    })
}

fn old_records() -> Vec<AbstractData> {
    let id = ArrayString::from("video").unwrap();
    vec![
        photo("image"),
        AbstractData::Video(VideoCombined {
            object: ObjectSchema::new(id, ObjectType::Video),
            metadata: VideoMetadata::new(id, 100, 32, 24, "mp4".into()),
        }),
        Album::new(ArrayString::from("album").unwrap(), None).into_abstract_data(),
    ]
}

/// This is the previous binary's typed V6 writer, without any audience tables.
fn write_old_database(path: &std::path::Path, records: &[AbstractData]) -> Result<()> {
    let db = Database::create(path)?;
    let txn = db.begin_write()?;
    {
        let mut table = txn.open_table(TableDefinition::<&str, V6AbstractData>::new("records"))?;
        for record in records {
            table.insert(record.hash().as_str(), V6AbstractData::from(record.clone()))?;
        }
    }
    txn.commit()?;
    Ok(())
}

#[test]
fn migration_is_one_time_and_preserves_frozen_v6_for_previous_reader() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("index_v6.redb");
    let records = old_records();
    write_old_database(&path, &records)?;
    let store = DataStore::open(&path)?;
    let before: Vec<_> = store
        .db
        .begin_read()?
        .open_table(RECORDS_TABLE)?
        .iter()?
        .map(|entry| entry.map(|(key, value)| (key.value().to_owned(), value.value().to_vec())))
        .collect::<redb::Result<_>>()?;
    assert_eq!(store.photo_audiences()?.get("image")?, Audience::Private);
    store.migrate_photo_audience()?;
    let revision = store.photo_audiences()?.revision;
    for hash in ["image", "video"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Public);
    }
    assert!(store.photo_audiences()?.table.get("album")?.is_none());

    // Every ingestion path uses these record writers; none writes audience rows.
    store.write(|writer| writer.insert(&photo("upload")))?;
    store.write_with_durability(Durability::Immediate, |writer| {
        writer.insert(&photo("folder-import"))
    })?;
    store.write_profiled(|writer| writer.insert(&photo("sync")))?;
    store.migrate_photo_audience()?;
    assert_eq!(store.photo_audiences()?.revision, revision);
    for hash in ["upload", "folder-import", "sync"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Private);
        assert!(store.photo_audiences()?.table.get(hash)?.is_none());
    }
    store.set_photo_audience("image", Audience::Private)?;
    drop(store);

    let store = DataStore::open(&path)?;
    store.migrate_photo_audience()?;
    assert_eq!(store.photo_audiences()?.get("image")?, Audience::Private);
    assert_eq!(store.photo_audiences()?.get("upload")?, Audience::Private);
    let after: Vec<_> = store
        .db
        .begin_read()?
        .open_table(RECORDS_TABLE)?
        .iter()?
        .filter_map(|entry| match entry {
            Ok((key, value))
                if records
                    .iter()
                    .any(|record| record.hash().as_str() == key.value()) =>
            {
                Some(Ok((key.value().to_owned(), value.value().to_vec())))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<redb::Result<_>>()?;
    assert_eq!(before, after);
    drop(store);

    // Open with the exact pre-audience typed table/type and decode every old record.
    let previous_reader = Database::open(path)?;
    let txn = previous_reader.begin_read()?;
    let table = txn.open_table(TableDefinition::<&str, V6AbstractData>::new("records"))?;
    for record in records {
        assert_eq!(
            table.get(record.hash().as_str())?.unwrap().value(),
            V6AbstractData::from(record)
        );
    }
    Ok(())
}

#[test]
fn corrupt_record_aborts_the_entire_migration_and_marker() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("index_v6.redb");
    write_old_database(&path, &[photo("a-valid")])?;
    let store = DataStore::open(&path)?;
    let txn = store.db.begin_write()?;
    txn.open_table(RECORDS_TABLE)?
        .insert("z-corrupt", b"invalid V6".as_slice())?;
    txn.commit()?;
    assert!(store.migrate_photo_audience().is_err());
    assert!(store.photo_audiences()?.table.get("a-valid")?.is_none());
    assert!(
        store
            .db
            .begin_read()?
            .open_table(AUDIENCE_META)?
            .get(MIGRATION)?
            .is_none()
    );
    store.write(|writer| writer.insert(&photo("z-corrupt")))?;
    store.migrate_photo_audience()?;
    assert_eq!(store.photo_audiences()?.get("a-valid")?, Audience::Public);
    assert_eq!(store.photo_audiences()?.get("z-corrupt")?, Audience::Public);
    Ok(())
}

#[test]
fn missing_unknown_and_explicit_private_entries_fail_closed() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    store.migrate_photo_audience()?;
    store.write(|writer| writer.insert(&photo("new")))?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    let txn = store.db.begin_write()?;
    txn.open_table(PHOTO_AUDIENCE)?.insert("new", 255)?;
    txn.commit()?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    let revision = store.photo_audiences()?.revision;
    store.set_photo_audience("new", Audience::Public)?;
    assert!(store.photo_audiences()?.revision > revision);
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Public);
    store.set_photo_audience("new", Audience::Private)?;
    store.migrate_photo_audience()?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    assert!(
        store
            .set_photo_audience("missing", Audience::Public)
            .is_err()
    );
    Ok(())
}

#[test]
fn new_database_is_marked_before_ingestion_and_restart_does_not_publish() -> Result<()> {
    let root = tempfile::tempdir()?;
    super::super::migration::prepare_storage_at(root.path())?;
    {
        let store = DataStore::open(&root.path().join("index_v6.redb"))?;
        store.write(|writer| writer.insert(&photo("new")))?;
    }
    super::super::migration::prepare_storage_at(root.path())?;
    let store = DataStore::open(&root.path().join("index_v6.redb"))?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    Ok(())
}

#[test]
fn updates_keep_audience_but_deletion_and_reingestion_discard_grants() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    store.migrate_photo_audience()?;
    store.write(|writer| writer.insert(&photo("same-hash")))?;
    store.set_photo_audience("same-hash", Audience::Public)?;
    store.write(|writer| writer.insert(&photo("same-hash")))?;
    assert_eq!(store.photo_audiences()?.get("same-hash")?, Audience::Public);

    // Record and audience deletion roll back together on a failed write.
    let failed = store.write(|writer| -> Result<()> {
        writer.remove("same-hash")?;
        bail!("fixture abort");
    });
    assert!(failed.is_err());
    assert_eq!(store.photo_audiences()?.get("same-hash")?, Audience::Public);
    store.write(|writer| writer.remove("same-hash"))?;
    assert!(store.photo_audiences()?.table.get("same-hash")?.is_none());

    // Model a dangling public entry left by a deletion in the previous binary.
    let txn = store.db.begin_write()?;
    txn.open_table(PHOTO_AUDIENCE)?.insert("same-hash", 1)?;
    txn.commit()?;
    store.write(|writer| writer.insert(&photo("same-hash")))?;
    assert!(store.photo_audiences()?.table.get("same-hash")?.is_none());
    assert_eq!(
        store.photo_audiences()?.get("same-hash")?,
        Audience::Private
    );
    Ok(())
}
