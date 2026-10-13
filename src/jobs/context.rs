//! Which job and which lane the running code belongs to, as Tokio task-local
//! values, so code far below the job (the HTTP backoff, a `tracing` warning)
//! can report to it without being handed it.

use super::live::{LiveJob, Wait};
use std::future::Future;
use std::sync::Arc;

tokio::task_local! {
    pub(super) static JOB: Arc<LiveJob>;
    static LANE: &'static str;
}

/// Run `work` as part of a lane: its waits and log lines are the lane's.
pub async fn in_lane<F: Future>(lane: &'static str, work: F) -> F::Output {
    use tracing::Instrument;
    LANE.scope(lane, work).instrument(tracing::info_span!(target: "upnext::timing", "lane", lane)).await
}

pub fn current_lane() -> Option<&'static str> {
    LANE.try_with(|l| *l).ok()
}

/// The id of the job running this code, if any.
pub fn current_job() -> Option<i64> {
    JOB.try_with(|j| j.id).ok()
}

/// Whether the job running this code was asked to stop (false outside a job).
pub fn stop_asked() -> bool {
    JOB.try_with(|j| crate::enrich::ProgressSink::cancelled(&**j)).unwrap_or(false)
}

/// A service made a request wait: tell the job, whose lane then says so.
/// False outside a job, where the caller should log it itself.
pub fn waiting(wait: &Wait) -> bool {
    JOB.try_with(|j| j.waiting(current_lane(), wait)).is_ok()
}

/// A warning or error from `tracing`, for the job running this code.
pub(super) fn traced(level: crate::db::Level, message: &str) -> Option<i64> {
    JOB.try_with(|j| {
        j.traced(level, message.to_string());
        j.id
    })
    .ok()
}
