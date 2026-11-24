//! Owner evaluation records, separate from tags, Rejections and derived caches.
use anyhow::{Result, bail};
use redb::{Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use super::DataStore;

const ROUNDS: TableDefinition<&str, &[u8]> = TableDefinition::new("acceptance_rounds_v1");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Judgment {
    Yes,
    No,
    Uncertain,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub concept: String,
    pub label: String,
    pub judgment: Option<Judgment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Snapshot flags and independent before/after review completion mirror the UI contract"
)]
pub struct ReviewPhoto {
    pub hash: String,
    pub version: u32,
    pub timestamp: i64,
    pub role: String,
    pub enabled: bool,
    pub pending: bool,
    pub before: Vec<Suggestion>,
    pub after: Option<Vec<Suggestion>>,
    pub seen_before: bool,
    pub seen_after: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRound {
    pub id: String,
    pub kind: String,
    pub captured_at: i64,
    pub model: String,
    pub vocabulary: String,
    pub materialized_at: i64,
    pub library_revision: i64,
    pub population: usize,
    pub plant: String,
    pub separation_confirmed: bool,
    pub after_captured_at: Option<i64>,
    pub photos: Vec<ReviewPhoto>,
    pub feedback: String,
    pub effort_seconds: u32,
}

impl DataStore {
    pub fn acceptance_round(&self, kind: &str) -> Result<Option<ReviewRound>> {
        let read = self.db.begin_read()?;
        let table = match read.open_table(ROUNDS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        table
            .get(kind)?
            .map(|value| serde_json::from_slice(value.value()))
            .transpose()
            .map_err(Into::into)
    }

    /// One immediate transaction: concurrent clicks cannot discard judgments.
    /// Old readers ignore this additive table; V6 and feedback stay untouched.
    pub fn update_acceptance(
        &self,
        kind: &str,
        update: impl FnOnce(Option<ReviewRound>) -> Result<ReviewRound>,
    ) -> Result<ReviewRound> {
        if !["random", "garden"].contains(&kind) {
            bail!("Unknown review kind");
        }
        let mut write = self.db.begin_write()?;
        write.set_durability(Durability::Immediate)?;
        let round;
        {
            let mut table = write.open_table(ROUNDS)?;
            let previous: Option<ReviewRound> = table
                .get(kind)?
                .map(|value| serde_json::from_slice(value.value()))
                .transpose()?;
            round = update(previous)?;
            let encoded = serde_json::to_vec(&round)?;
            table.insert(kind, encoded.as_slice())?;
            table.insert(round.id.as_str(), encoded.as_slice())?;
        }
        write.commit()?;
        Ok(round)
    }
}

#[cfg(test)]
mod tests;
