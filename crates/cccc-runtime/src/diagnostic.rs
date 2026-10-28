//! Explicit diagnostics can leave the terminal to the foreground interface.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;

struct Log {
    file: File,
    failed: bool,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Owns diagnostic routing for one foreground terminal session.
pub struct DiagnosticLog;

pub fn log_diagnostics_to(path: &Path) -> io::Result<DiagnosticLog> {
    let mut log = LOG.lock().unwrap_or_else(|error| error.into_inner());
    if log.is_some() {
        return Err(io::Error::other(
            "terminal diagnostics are already redirected",
        ));
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    *log = Some(Log {
        file: options.open(path)?,
        failed: false,
    });
    Ok(DiagnosticLog)
}

pub fn diagnostic(message: fmt::Arguments<'_>) {
    let mut log = LOG.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(log) = log.as_mut() {
        if writeln!(log.file, "{} {message}", cccc_contracts::utc_now()).is_err() {
            log.failed = true;
        }
    } else {
        drop(log);
        eprintln!("{message}");
    }
}

impl Drop for DiagnosticLog {
    fn drop(&mut self) {
        let log = LOG.lock().unwrap_or_else(|error| error.into_inner()).take();
        if log.is_some_and(|log| log.failed) {
            eprintln!("CCCC could not save some diagnostics to its log file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_diagnostics_are_appended_and_a_second_owner_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("host.log");
        std::fs::write(&path, "existing diagnostic\n").expect("existing log");
        let guard = log_diagnostics_to(&path).expect("route diagnostics");
        assert!(log_diagnostics_to(&path).is_err());
        diagnostic(format_args!("provider stopped: {}", "fixture"));
        drop(guard);
        let text = std::fs::read_to_string(&path).expect("log");
        assert!(text.starts_with("existing diagnostic\n"));
        assert!(text.contains("provider stopped: fixture\n"));
        drop(log_diagnostics_to(&path).expect("previous owner released"));
    }
}
