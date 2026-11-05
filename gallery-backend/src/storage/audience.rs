//! Separate redb tables preserve the frozen V6 record bytes and old-reader compatibility.
use anyhow::{Context, Result, bail};
use redb::{Durability, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition};

use super::{DataStore, store::RECORDS_TABLE};
use crate::public::structure::{abstract_data::AbstractData, audience::Audience};

pub(super) const PHOTO_AUDIENCE: TableDefinition<&str, u8> = TableDefinition::new("audience");
const AUDIENCE_META: TableDefinition<&str, u64> = TableDefinition::new("audience_meta");
const MIGRATION: &str = "existing_photos_public_v1";
const ALBUM_MIGRATION: &str = "existing_albums_public_v1";
const REVISION: &str = "revision";

pub struct PhotoAudiences {
    table: ReadOnlyTable<&'static str, u8>,
    pub revision: u64,
}

impl PhotoAudiences {
    pub fn get(&self, hash: &str) -> Result<Audience> {
        // Missing and unknown values fail closed, including future enum values.
        Ok(
            if self
                .table
                .get(hash)?
                .is_some_and(|entry| entry.value() == 1)
            {
                Audience::Public
            } else {
                Audience::Private
            },
        )
    }
}

impl DataStore {
    pub(super) fn initialize_audience_tables(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(PHOTO_AUDIENCE)?;
        txn.open_table(AUDIENCE_META)?;
        txn.commit()?;
        Ok(())
    }

    pub fn photo_audiences(&self) -> Result<PhotoAudiences> {
        let txn = self.db.begin_read()?;
        let revision = txn
            .open_table(AUDIENCE_META)?
            .get(REVISION)?
            .map_or(0, |v| v.value());
        Ok(PhotoAudiences {
            table: txn.open_table(PHOTO_AUDIENCE)?,
            revision,
        })
    }

    /// Run before ingestion or HTTP startup. The marker and all entries commit atomically.
    pub(crate) fn migrate_photo_audience(&self) -> Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        {
            let mut meta = txn.open_table(AUDIENCE_META)?;
            if meta.get(MIGRATION)?.is_some() {
                return Ok(());
            }
            let records = txn.open_table(RECORDS_TABLE)?;
            let mut audiences = txn.open_table(PHOTO_AUDIENCE)?;
            for entry in records.iter()? {
                let (hash, bytes) = entry?;
                let record = bitcode::decode::<super::v6::V6AbstractData>(bytes.value())
                    .with_context(|| {
                        format!(
                            "failed to decode V6 record {} for audience migration",
                            hash.value()
                        )
                    })?;
                if !matches!(record, super::v6::V6AbstractData::Album(_))
                    && audiences.get(hash.value())?.is_none()
                {
                    audiences.insert(hash.value(), 1)?;
                }
            }
            let revision = meta
                .get(REVISION)?
                .map_or(0, |v| v.value())
                .checked_add(1)
                .context("photo audience revision exhausted")?;
            meta.insert(REVISION, revision)?;
            meta.insert(MIGRATION, 1)?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Independent of photo migration: only pre-existing albums receive a grant.
    pub(crate) fn migrate_album_audience(&self) -> Result<()> {
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        {
            let mut meta = txn.open_table(AUDIENCE_META)?;
            if meta.get(ALBUM_MIGRATION)?.is_some() {
                return Ok(());
            }
            let records = txn.open_table(RECORDS_TABLE)?;
            let mut audiences = txn.open_table(PHOTO_AUDIENCE)?;
            for entry in records.iter()? {
                let (hash, bytes) = entry?;
                let record = bitcode::decode::<super::v6::V6AbstractData>(bytes.value())
                    .with_context(|| {
                        format!(
                            "failed to decode V6 record {} for album audience migration",
                            hash.value()
                        )
                    })?
                    .into_domain()?;
                if matches!(record, AbstractData::Album(_))
                    && audiences.get(hash.value())?.is_none()
                {
                    audiences.insert(hash.value(), 1)?;
                }
            }
            let revision = meta
                .get(REVISION)?
                .map_or(0, |v| v.value())
                .checked_add(1)
                .context("album audience revision exhausted")?;
            meta.insert(REVISION, revision)?;
            meta.insert(ALBUM_MIGRATION, 1)?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Change the album only, unless the explicit action also publishes its current
    /// non-trashed photos/videos. Callers must first persist pending membership edits.
    pub fn set_album_audience(
        &self,
        hash: &str,
        audience: Audience,
        publish_photos: bool,
    ) -> Result<()> {
        if publish_photos && audience != Audience::Public {
            bail!("publishing album photos requires a public album");
        }
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        {
            let records = txn.open_table(RECORDS_TABLE)?;
            let bytes = records.get(hash)?.context("album not found")?;
            let AbstractData::Album(album) =
                bitcode::decode::<super::v6::V6AbstractData>(bytes.value())?.into_domain()?
            else {
                bail!("record is not an album");
            };
            if album.object.is_trashed {
                bail!("album is trashed");
            }
            let mut audiences = txn.open_table(PHOTO_AUDIENCE)?;
            audiences.insert(hash, u8::from(audience == Audience::Public))?;
            if publish_photos {
                for entry in records.iter()? {
                    let (member_hash, bytes) = entry?;
                    let record = bitcode::decode::<super::v6::V6AbstractData>(bytes.value())?
                        .into_domain()?;
                    if !record.is_trashed()
                        && record.albums().is_some_and(|albums| albums.contains(hash))
                    {
                        audiences.insert(member_hash.value(), 1)?;
                    }
                }
            }
            let mut meta = txn.open_table(AUDIENCE_META)?;
            let revision = meta
                .get(REVISION)?
                .map_or(0, |v| v.value())
                .checked_add(1)
                .context("album audience revision exhausted")?;
            meta.insert(REVISION, revision)?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Publishing is explicit; normal record writes never insert an audience entry.
    pub fn set_photo_audience(&self, hash: &str, audience: Audience) -> Result<()> {
        self.set_photo_audiences(&[hash], audience)
    }

    /// Validate every photo and commit a batch with one durable audience revision.
    pub fn set_photo_audiences(&self, hashes: &[&str], audience: Audience) -> Result<()> {
        if hashes.is_empty() {
            return Ok(());
        }
        let mut txn = self.db.begin_write()?;
        txn.set_durability(Durability::Immediate)?;
        {
            let records = txn.open_table(RECORDS_TABLE)?;
            let mut audiences = txn.open_table(PHOTO_AUDIENCE)?;
            for hash in hashes {
                let bytes = records.get(*hash)?.context("photo not found")?;
                if matches!(
                    bitcode::decode::<super::v6::V6AbstractData>(bytes.value())?,
                    super::v6::V6AbstractData::Album(_)
                ) {
                    bail!("album audience is managed separately");
                }
                audiences.insert(*hash, u8::from(audience == Audience::Public))?;
            }
            let mut meta = txn.open_table(AUDIENCE_META)?;
            let revision = meta
                .get(REVISION)?
                .map_or(0, |v| v.value())
                .checked_add(1)
                .context("photo audience revision exhausted")?;
            meta.insert(REVISION, revision)?;
        }
        txn.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
