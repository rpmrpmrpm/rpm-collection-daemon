//! A small logger that writes to standard error. A NAS system collects standard error into its own
//! log, so the daemon keeps no log file.

use std::io::Write;

use log::{LevelFilter, Log, Metadata, Record};

struct StandardError {
    level: LevelFilter,
}

impl Log for StandardError {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        // Only this program's own messages, and warnings from the libraries it uses.
        let ours = record.target().starts_with("rpm_");
        if self.enabled(record.metadata()) && (ours || record.level() <= log::Level::Warn) {
            let _ = writeln!(std::io::stderr(), "{} {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Starts logging at `level` (`error`, `warn`, `info`, `debug` or `trace`; `info` when not set or not
/// known).
pub fn init(level: Option<&str>) {
    let level = level
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(LevelFilter::Info);
    if log::set_boxed_logger(Box::new(StandardError { level })).is_ok() {
        log::set_max_level(level);
    }
}
