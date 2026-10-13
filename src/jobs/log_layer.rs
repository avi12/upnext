//! A `tracing` layer that keeps UpNext's warnings and errors where the UI can
//! show them: in the log of the job that raised them, and in a buffer of the
//! process's recent ones (`GET /api/logs`) for failures outside any job.

use crate::db::{Level, now};
use serde::Serialize;
use std::collections::VecDeque;
use std::fmt::Write;
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::Context;

/// Lines kept in the recent buffer.
const RECENT: usize = 300;

static LINES: Mutex<VecDeque<ServerLine>> = Mutex::new(VecDeque::new());

/// A warning or error the process logged.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ServerLine {
    pub at: i64,
    pub level: Level,
    pub message: String,
    /// The job it came from, if it came from one.
    pub job_id: Option<i64>,
}

/// The layer, filtered to UpNext's own warnings and errors.
pub fn layer<S>() -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    LogLayer.with_filter(Targets::new().with_target("upnext", LevelFilter::WARN))
}

struct LogLayer;

impl<S: tracing::Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let level = match *event.metadata().level() {
            tracing::Level::ERROR => Level::Error,
            tracing::Level::WARN => Level::Warn,
            _ => return,
        };
        let mut message = Message::default();
        event.record(&mut message);
        let job_id = super::context::traced(level, &message.0);
        remember(ServerLine { at: now(), level, message: message.0, job_id });
    }
}

fn remember(line: ServerLine) {
    let mut lines = LINES.lock().unwrap_or_else(|e| e.into_inner());
    if lines.len() == RECENT {
        lines.pop_front();
    }
    lines.push_back(line);
}

/// The process's recent warnings and errors at `min` or above, oldest first.
pub fn recent(min: Level) -> Vec<ServerLine> {
    LINES.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|l| l.level >= min).cloned().collect()
}

/// The event's message, then its other fields as `name=value`.
#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.insert_str(0, value);
        } else {
            let _ = write!(self.0, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0.insert_str(0, &format!("{value:?}"));
        } else {
            let _ = write!(self.0, " {}={value:?}", field.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn warnings_are_kept_with_their_fields_and_info_is_not() {
        let subscriber = tracing_subscriber::registry().with(layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("layer test: not kept");
            tracing::warn!(status = 429, "layer test: slow down");
            tracing::error!("layer test: broken {}", 7);
        });
        let mine: Vec<(Level, String)> =
            recent(Level::Warn).into_iter().filter(|l| l.message.starts_with("layer test")).map(|l| (l.level, l.message)).collect();
        assert_eq!(
            mine,
            [(Level::Warn, "layer test: slow down status=429".to_string()), (Level::Error, "layer test: broken 7".to_string())]
        );
        assert!(recent(Level::Error).iter().all(|l| l.level == Level::Error));
    }
}
