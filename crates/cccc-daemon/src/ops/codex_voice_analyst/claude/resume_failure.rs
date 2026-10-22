//! Safe diagnostics at the resume boundary; never persist provider stderr or credentials.
use std::{fmt, io};

#[derive(Debug)]
pub(super) enum Rejection {
    CopiedSession,
    MissingHistory,
}

impl Rejection {
    pub(super) fn error(self) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, self)
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CopiedSession => {
                "Claude Agent View copied a requested session instead of resuming it exactly"
            }
            Self::MissingHistory => "Claude Agent View did not expose its durable transcript",
        })
    }
}

impl std::error::Error for Rejection {}

#[derive(Debug)]
pub(super) struct CleanupFailure {
    pub(super) primary: io::Error,
    pub(super) cleanup: io::Error,
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}; cleanup also failed: {}", self.primary, self.cleanup)
    }
}

impl std::error::Error for CleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.primary)
    }
}

/// `None` is an operator trust decision, not evidence about the saved conversation.
/// Known transient control failures retain resume eligibility; other unverifiable
/// failures pause automatic recovery without asserting that history has been lost.
pub(in crate::ops::codex_voice_analyst) fn diagnostic(
    error: &io::Error,
) -> Option<(&'static str, bool)> {
    if super::untrusted_workspace(error).is_some() {
        return None;
    }
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<CleanupFailure>())
    {
        return Some((
            "The unsuccessful Claude resume could not be cleaned up. Check the provider before retrying.",
            true,
        ));
    }
    if let Some(rejection) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Rejection>())
    {
        return Some((
            match rejection {
                Rejection::CopiedSession => {
                    "Claude returned a different session ID. CCCC rejected the copied session and preserved the saved conversation."
                }
                Rejection::MissingHistory => {
                    "Claude did not expose the saved conversation's durable transcript. CCCC could not verify its history."
                }
            },
            true,
        ));
    }
    if let Some(diagnostic) = transient(error.kind()) {
        return Some((diagnostic, false));
    }
    let diagnostic = match error.kind() {
        io::ErrorKind::NotFound => "A file or executable required to resume Claude is unavailable.",
        io::ErrorKind::PermissionDenied => {
            "Claude resume failed a permission or credential-boundary check."
        }
        io::ErrorKind::Unsupported => {
            "The installed Claude worker or control protocol is not supported."
        }
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            "CCCC could not verify Claude's saved session identity, history or control protocol."
        }
        _ => {
            "Claude could not resume the saved conversation. Check the provider configuration before retrying."
        }
    };
    Some((diagnostic, true))
}

/// Name the error kind of a transient failure from a fixed vocabulary, so an
/// operator can tell a timeout from a refused or reset control connection
/// without CCCC persisting any provider text.
fn transient(kind: io::ErrorKind) -> Option<&'static str> {
    macro_rules! transient {
        ($kind:literal) => {
            concat!(
                "Claude resume encountered a temporary control or startup failure (kind=",
                $kind,
                "). The saved conversation remains eligible for retry."
            )
        };
    }
    Some(match kind {
        io::ErrorKind::TimedOut => transient!("timed_out"),
        io::ErrorKind::WouldBlock => transient!("would_block"),
        io::ErrorKind::Interrupted => transient!("interrupted"),
        io::ErrorKind::ConnectionAborted => transient!("connection_aborted"),
        io::ErrorKind::ConnectionRefused => transient!("connection_refused"),
        io::ErrorKind::ConnectionReset => transient!("connection_reset"),
        io::ErrorKind::BrokenPipe => transient!("broken_pipe"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_diagnostics_do_not_copy_provider_errors_or_credential_values() {
        for kind in [
            io::ErrorKind::Other,
            io::ErrorKind::InvalidData,
            io::ErrorKind::TimedOut,
        ] {
            let error = io::Error::new(kind, "ANTHROPIC_API_KEY=synthetic-secret provider stderr");
            let (safe, blocked) = diagnostic(&error).expect("failure");
            assert!(!safe.contains("synthetic-secret"));
            assert!(!safe.contains("ANTHROPIC_API_KEY"));
            assert_eq!(blocked, kind != io::ErrorKind::TimedOut);
        }
        for error in [
            Rejection::CopiedSession.error(),
            Rejection::MissingHistory.error(),
        ] {
            assert!(diagnostic(&error).expect("identity/history rejection").1);
        }
        let error = io::Error::new(
            io::ErrorKind::TimedOut,
            CleanupFailure {
                primary: io::Error::new(io::ErrorKind::TimedOut, "synthetic-secret"),
                cleanup: io::Error::other("synthetic-secret"),
            },
        );
        let (safe, blocked) = diagnostic(&error).expect("cleanup failure");
        assert!(
            blocked,
            "unconfirmed cleanup must not start another job automatically"
        );
        assert!(!safe.contains("synthetic-secret"));
    }

    #[test]
    fn transient_resume_diagnostics_name_the_error_kind_and_nothing_else() {
        for (kind, token) in [
            (io::ErrorKind::TimedOut, "timed_out"),
            (io::ErrorKind::WouldBlock, "would_block"),
            (io::ErrorKind::Interrupted, "interrupted"),
            (io::ErrorKind::ConnectionAborted, "connection_aborted"),
            (io::ErrorKind::ConnectionRefused, "connection_refused"),
            (io::ErrorKind::ConnectionReset, "connection_reset"),
            (io::ErrorKind::BrokenPipe, "broken_pipe"),
        ] {
            let error = io::Error::new(
                kind,
                "/home/user/.claude/daemon/control.sock ANTHROPIC_API_KEY=synthetic-secret",
            );
            let (safe, blocked) = diagnostic(&error).expect("transient failure");
            assert!(!blocked, "{kind:?} keeps automatic recovery");
            assert!(safe.contains(&format!("(kind={token})")), "{safe}");
            assert!(safe.contains("remains eligible for retry"), "{safe}");
            assert!(!safe.contains("synthetic-secret"), "{safe}");
            assert!(!safe.contains("control.sock"), "{safe}");
        }
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::Other] {
            let (safe, blocked) = diagnostic(&io::Error::new(kind, "x")).expect("failure");
            assert!(blocked, "{kind:?} still pauses automatic recovery");
            assert!(!safe.contains("kind="), "{safe}");
        }
    }
}
