use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use futures::FutureExt;
use serde::Serialize;
use uuid::Uuid;

use crate::operations::video_metadata::{StripReport, strip_compressed_videos};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StripJobState {
    Running,
    Completed,
    CompletedWithErrors,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripJobStatus {
    pub job_id: String,
    pub state: StripJobState,
    pub report: Option<StripReport>,
    pub error: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StripJobAccepted {
    pub job_id: String,
}

// Only the latest run is retained. Restarting safely rechecks every compressed file.
static STRIP_JOB: LazyLock<Mutex<Option<StripJobStatus>>> = LazyLock::new(|| Mutex::new(None));

pub fn strip_job_status() -> Option<StripJobStatus> {
    STRIP_JOB.lock().unwrap().clone()
}

/// A missing result means a run is already active. The caller supplies the
/// server's configured data root, never a path from an HTTP request.
pub fn start_strip_job(root: PathBuf) -> Option<StripJobAccepted> {
    let mut current = STRIP_JOB.lock().unwrap();
    if current
        .as_ref()
        .is_some_and(|job| job.state == StripJobState::Running)
    {
        return None;
    }
    let job_id = Uuid::new_v4().to_string();
    *current = Some(StripJobStatus {
        job_id: job_id.clone(),
        state: StripJobState::Running,
        report: None,
        error: None,
    });
    // Dropping this handle does not cancel the task when the request ends.
    tokio::spawn(async move {
        let result = AssertUnwindSafe(strip_compressed_videos(&root))
            .catch_unwind()
            .await;
        let mut current = STRIP_JOB.lock().unwrap();
        let job = current.as_mut().expect("running strip job is registered");
        match result {
            Ok(Ok(report)) => {
                job.state = if report.failed > 0 || !report.verification.passed {
                    StripJobState::CompletedWithErrors
                } else {
                    StripJobState::Completed
                };
                job.report = Some(report);
            }
            Ok(Err(_)) => {
                job.state = StripJobState::Failed;
                job.error = Some("Compressed-video metadata stripping failed; rerun is safe");
            }
            Err(_) => {
                job.state = StripJobState::Failed;
                job.error = Some("Compressed-video metadata worker stopped; rerun is safe");
            }
        }
    });
    Some(StripJobAccepted { job_id })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{StripJobState, start_strip_job, strip_job_status};

    #[tokio::test]
    async fn concurrent_start_is_refused_and_completed_run_can_restart() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("object/compressed")).unwrap();
        let accepted = start_strip_job(root.path().to_owned()).unwrap();
        // Tokio does not poll a spawned task synchronously; this is the active run.
        assert!(start_strip_job(root.path().to_owned()).is_none());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status = strip_job_status().unwrap();
                assert_eq!(status.job_id, accepted.job_id);
                if status.state != StripJobState::Running {
                    assert_eq!(status.state, StripJobState::Completed);
                    assert_eq!(status.report.unwrap().total, 0);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let second = start_strip_job(root.path().to_owned()).unwrap();
        assert_ne!(second.job_id, accepted.job_id);
        tokio::time::timeout(Duration::from_secs(10), async {
            while strip_job_status().unwrap().state == StripJobState::Running {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
