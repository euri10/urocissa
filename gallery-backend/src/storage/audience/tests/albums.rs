use super::*;

fn album(id: &str) -> AbstractData {
    Album::new(ArrayString::from(id).unwrap(), None).into_abstract_data()
}

fn record_bytes(store: &DataStore) -> Result<Vec<(String, Vec<u8>)>> {
    Ok(store
        .db
        .begin_read()?
        .open_table(RECORDS_TABLE)?
        .iter()?
        .map(|entry| entry.map(|(key, value)| (key.value().to_owned(), value.value().to_vec())))
        .collect::<redb::Result<_>>()?)
}

#[test]
fn album_migration_is_independent_idempotent_and_preserves_v6_bytes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("index_v6.redb");
    let mut records = old_records();
    records.extend([album("explicit-private"), album("unknown")]);
    write_old_database(&path, &records)?;
    let store = DataStore::open(&path)?;
    store.migrate_photo_audience()?;
    store.set_photo_audience("image", Audience::Private)?;
    store.write(|writer| writer.insert(&photo("later-photo")))?;
    let txn = store.db.begin_write()?;
    {
        let mut audiences = txn.open_table(PHOTO_AUDIENCE)?;
        audiences.insert("explicit-private", 0)?;
        audiences.insert("unknown", 255)?;
    }
    txn.commit()?;
    let before = record_bytes(&store)?;
    let revision = store.photo_audiences()?.revision;
    store.migrate_album_audience()?;
    assert_eq!(store.photo_audiences()?.revision, revision + 1);
    assert_eq!(store.photo_audiences()?.get("album")?, Audience::Public);
    for hash in ["image", "later-photo", "explicit-private", "unknown"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Private);
    }
    assert_eq!(store.photo_audiences()?.get("video")?, Audience::Public);
    assert_eq!(record_bytes(&store)?, before);
    assert_eq!(
        store
            .photo_audiences()?
            .table
            .get("unknown")?
            .unwrap()
            .value(),
        255
    );

    store.write(|writer| writer.insert(&album("new-album")))?;
    store.set_album_audience("album", Audience::Private, false)?;
    let revision = store.photo_audiences()?.revision;
    drop(store);
    super::super::super::migration::prepare_storage_at(root.path())?;
    let store = DataStore::open(&path)?;
    assert_eq!(store.photo_audiences()?.revision, revision);
    for hash in ["album", "new-album", "image", "later-photo"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Private);
    }
    drop(store);

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
fn corrupt_record_rolls_back_album_migration_and_retry_does_not_publish_photos() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("index_v6.redb");
    write_old_database(&path, &[album("a-album")])?;
    let store = DataStore::open(&path)?;
    store.migrate_photo_audience()?;
    let revision = store.photo_audiences()?.revision;
    let txn = store.db.begin_write()?;
    txn.open_table(RECORDS_TABLE)?
        .insert("z-corrupt", b"invalid V6".as_slice())?;
    txn.commit()?;

    assert!(store.migrate_album_audience().is_err());
    assert!(store.photo_audiences()?.table.get("a-album")?.is_none());
    assert_eq!(store.photo_audiences()?.revision, revision);
    assert!(
        store
            .db
            .begin_read()?
            .open_table(AUDIENCE_META)?
            .get(ALBUM_MIGRATION)?
            .is_none()
    );
    store.write(|writer| writer.insert(&photo("z-corrupt")))?;
    store.migrate_album_audience()?;
    assert_eq!(store.photo_audiences()?.get("a-album")?, Audience::Public);
    assert_eq!(
        store.photo_audiences()?.get("z-corrupt")?,
        Audience::Private
    );
    assert_eq!(store.photo_audiences()?.revision, revision + 1);
    Ok(())
}

#[test]
fn fresh_database_marks_albums_before_creation_and_deletion_discards_grants() -> Result<()> {
    let root = tempfile::tempdir()?;
    super::super::super::migration::prepare_storage_at(root.path())?;
    let path = root.path().join("index_v6.redb");
    let store = DataStore::open(&path)?;
    store.write(|writer| writer.insert(&album("new")))?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    store.set_album_audience("new", Audience::Public, false)?;
    store.write(|writer| writer.insert(&album("new")))?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Public);
    store.write(|writer| writer.remove("new"))?;
    assert!(store.photo_audiences()?.table.get("new")?.is_none());
    store.write(|writer| writer.insert(&album("new")))?;
    drop(store);
    super::super::super::migration::prepare_storage_at(root.path())?;
    let store = DataStore::open(&path)?;
    assert_eq!(store.photo_audiences()?.get("new")?, Audience::Private);
    Ok(())
}

#[test]
fn album_actions_only_publish_photos_when_explicit_and_never_inherit_audience() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    store.migrate_photo_audience()?;
    store.migrate_album_audience()?;
    let mut records = old_records();
    for record in &mut records {
        if let Some(albums) = record.albums_mut() {
            albums.insert(ArrayString::from("album").unwrap());
        }
    }
    let mut trashed = photo("trashed");
    trashed
        .albums_mut()
        .unwrap()
        .insert(ArrayString::from("album").unwrap());
    if let AbstractData::Image(image) = &mut trashed {
        image.object.is_trashed = true;
    }
    records.extend([trashed, photo("other")]);
    store.write(|writer| {
        for record in &records {
            writer.insert(record)?;
        }
        Ok::<_, anyhow::Error>(())
    })?;
    let before = record_bytes(&store)?;
    let revision = store.photo_audiences()?.revision;
    store.set_album_audience("album", Audience::Public, false)?;
    assert_eq!(store.photo_audiences()?.revision, revision + 1);
    assert_eq!(store.photo_audiences()?.get("album")?, Audience::Public);
    for hash in ["image", "video", "trashed", "other"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Private);
    }
    store.set_album_audience("album", Audience::Public, true)?;
    assert_eq!(store.photo_audiences()?.revision, revision + 2);
    for hash in ["album", "image", "video"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Public);
    }
    for hash in ["trashed", "other"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Private);
    }
    assert_eq!(record_bytes(&store)?, before);

    let mut added = photo("added-later");
    added
        .albums_mut()
        .unwrap()
        .insert(ArrayString::from("album").unwrap());
    store.write(|writer| writer.insert(&added))?;
    assert_eq!(
        store.photo_audiences()?.get("added-later")?,
        Audience::Private
    );
    store.set_album_audience("album", Audience::Private, false)?;
    assert_eq!(store.photo_audiences()?.get("album")?, Audience::Private);
    for hash in ["image", "video"] {
        assert_eq!(store.photo_audiences()?.get(hash)?, Audience::Public);
    }
    Ok(())
}

#[test]
fn invalid_or_corrupt_album_publication_is_atomic() -> Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    let mut member = photo("a-member");
    member
        .albums_mut()
        .unwrap()
        .insert(ArrayString::from("album").unwrap());
    store.write(|writer| {
        writer.insert(&album("album"))?;
        writer.insert(&member)
    })?;
    let revision = store.photo_audiences()?.revision;
    for (hash, audience, publish_photos) in [
        ("missing", Audience::Public, false),
        ("a-member", Audience::Public, true),
        ("album", Audience::Private, true),
    ] {
        assert!(
            store
                .set_album_audience(hash, audience, publish_photos)
                .is_err()
        );
    }
    let txn = store.db.begin_write()?;
    txn.open_table(RECORDS_TABLE)?
        .insert("z-corrupt", b"invalid V6".as_slice())?;
    txn.commit()?;
    assert!(
        store
            .set_album_audience("album", Audience::Public, true)
            .is_err()
    );
    for hash in ["album", "a-member"] {
        assert!(store.photo_audiences()?.table.get(hash)?.is_none());
    }
    assert_eq!(store.photo_audiences()?.revision, revision);
    Ok(())
}
