//! Completion metadata is disposable and starts unknown after a restart.
use std::{collections::HashSet, sync::TryLockError, sync::atomic::Ordering};

use anyhow::Result;
use serde::Serialize;

use super::AutoTags;
use crate::{
    public::db::tree::{TREE, VERSION_COUNT_TIMESTAMP},
    storage::auto_tags::{MaterializedTags, StoredAutoTag},
};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub added: usize,
    pub removed: usize,
}

impl Changes {
    pub(super) fn compare(&mut self, previous: Option<&MaterializedTags>, next: &[StoredAutoTag]) {
        let before = previous
            .into_iter()
            .filter(|row| row.enabled)
            .flat_map(|row| &row.tags)
            .filter(|tag| tag.displayed)
            .map(|tag| tag.concept.as_str())
            .collect::<HashSet<_>>();
        let after = next
            .iter()
            .filter(|tag| tag.displayed)
            .map(|tag| tag.concept.as_str())
            .collect::<HashSet<_>>();
        self.added += after.difference(&before).count();
        self.removed += before.difference(&after).count();
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecalculationStatus {
    pub running: bool,
    pub up_to_date: bool,
    pub last_completed_at: Option<i64>,
    pub changes: Changes,
    pub last_error: Option<String>,
    #[serde(skip)]
    revision: Option<(i64, u64)>,
}

impl AutoTags {
    pub fn recalculation_status(&self) -> RecalculationStatus {
        let mut status = self.recalculation.lock().unwrap().clone();
        status.up_to_date = !status.running
            && status.last_error.is_none()
            && status.last_completed_at.is_some()
            && status.revision == Some(self.current_revision());
        status
    }

    fn current_revision(&self) -> (i64, u64) {
        (
            VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed),
            self.input_revision.load(Ordering::Relaxed),
        )
    }

    /// Manual requests never queue a second recalculation behind an active one.
    pub fn recalculate(&self) -> Result<bool> {
        let _run = match self.recalculation_lock.try_lock() {
            Ok(run) => run,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => anyhow::bail!("recalculation lock poisoned"),
        };
        self.reconcile_locked(true)?;
        Ok(true)
    }

    pub(super) fn reconcile_locked(&self, force: bool) -> Result<()> {
        self.recalculation.lock().unwrap().running = true;
        let result = (|| {
            // Authored decisions and derived commits keep the existing lock order.
            let _persistence = TREE
                .persistence_lock
                .lock()
                .map_err(|_| anyhow::anyhow!("tree persistence lock poisoned"))?;
            // Capture BEFORE the snapshot: concurrent edits must leave status pending.
            let revision = self.current_revision();
            let (inputs, counts) = super::gallery_inputs()?;
            if force {
                let mut guard = self.prepared.lock().unwrap();
                let prepared = guard
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Auto tags are not ready"))?;
                prepared.learning_dirty = true;
                prepared.seen.clear();
            }
            let changes = self.materialize(&inputs, &counts)?;
            Ok::<_, anyhow::Error>((revision, changes))
        })();
        let mut status = self.recalculation.lock().unwrap();
        status.running = false;
        match result {
            Ok((revision, changes)) => {
                if let Some(changes) = changes {
                    status.last_completed_at = Some(chrono::Utc::now().timestamp_millis());
                    status.changes = changes;
                }
                status.revision = Some(revision);
                status.last_error = None;
                Ok(())
            }
            Err(error) => {
                status.last_error =
                    Some("Could not recalculate Auto tags. Please try again.".into());
                // Failed partial work must be retried, never treated as already seen.
                if let Some(prepared) = self.prepared.lock().unwrap().as_mut() {
                    prepared.seen.clear();
                }
                Err(error)
            }
        }
    }
}
