use super::*;
use crate::storage::DataStore;

#[test]
fn retained_single_key_undo_requires_read_only_rollback() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    store.write(|writer| {
        write_rejection(writer, "photo", "snow", false)?;
        // Exact retained afebc3e7 ClearRejection behavior; it lacks alias deletion.
        writer.clear_auto_tag_rejection("photo", "snow")?;
        Ok::<_, anyhow::Error>(())
    })?;
    assert_eq!(
        canonical_rejections(store.auto_tag_rejections_for("photo")?),
        ["snow"],
        "old writer Undo is not forward-safe: recovery must prevent these writes"
    );
    store.write(|writer| write_rejection(writer, "photo", "snow", true))?;
    assert!(store.auto_tag_rejections_for("photo")?.is_empty());
    Ok(())
}

#[test]
fn legacy_rejections_conflict_and_undo_without_changing_unknown_decisions() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let store = DataStore::initialize_empty(&root.path().join("index_v6.redb"))?;
    let legacy = personal_id(" DOG ");
    let unrelated = personal_id("unrelated personal name");
    store.write(|writer| {
        writer.reject_auto_tag("old-photo", &legacy)?;
        writer.reject_auto_tag("old-photo", &unrelated)?;
        assert!(rejected_by(writer, "old-photo", "dog")?);
        assert!(!rejected_by(writer, "old-photo", "cat")?);
        write_rejection(writer, "new-photo", "dog", false)?;
        Ok::<_, anyhow::Error>(())
    })?;
    let old = canonical_rejections(store.auto_tag_rejections_for("old-photo")?);
    assert_eq!(old.len(), 2);
    assert!(old.contains(&"dog".into()) && old.contains(&unrelated));
    // The unchanged previous reader can still find its personal-ID table key.
    let backwards = store.auto_tag_rejections_for("new-photo")?;
    assert!(backwards.contains(&legacy) && backwards.contains(&"dog".into()));
    assert_eq!(canonical_rejections(backwards), ["dog"]);
    store.write(|writer| {
        write_rejection(writer, "old-photo", "dog", true)?;
        write_rejection(writer, "new-photo", "dog", true)?;
        assert!(!rejected_by(writer, "old-photo", "dog")?);
        Ok::<_, anyhow::Error>(())
    })?;
    assert_eq!(store.auto_tag_rejections_for("old-photo")?, [unrelated]);
    assert!(store.auto_tag_rejections_for("new-photo")?.is_empty());
    assert_eq!(store.record_count()?, 0);
    Ok(())
}
