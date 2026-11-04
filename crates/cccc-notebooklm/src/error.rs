use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("NotebookLM credentials are invalid: {0}")]
    InvalidCredential(String),
    #[error("NotebookLM authentication expired or was rejected")]
    Authentication,
    #[error("NotebookLM request was refused: {0}")]
    Refused(String),
    #[error("NotebookLM request was rate limited: {0}")]
    RateLimited(String),
    #[error("NotebookLM operation timed out: {0}")]
    Timeout(String),
    #[error("NotebookLM operation has an unresolved remote outcome: {0}")]
    Unresolved(String),
    #[error("NotebookLM transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("NotebookLM RPC {rpc_id} failed: {message}")]
    Rpc { rpc_id: String, message: String },
    #[error("NotebookLM response schema changed at {context}: {message}")]
    SchemaDrift {
        context: &'static str,
        message: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn drift(context: &'static str, message: impl Into<String>) -> Self {
        Self::SchemaDrift {
            context,
            message: message.into(),
        }
    }

    /// Whether a failed create may still have taken effect remotely. Sign-in
    /// failures, rate limits, refusals and HTTP 4xx answers other than 408 are
    /// definite rejections.
    pub(crate) fn may_have_committed(&self) -> bool {
        match self {
            Self::Transport(error) => !rejected_by_http_status(error),
            Self::Rpc { .. }
            | Self::SchemaDrift { .. }
            | Self::Timeout(_)
            | Self::Unresolved(_) => true,
            Self::InvalidCredential(_)
            | Self::Authentication
            | Self::Refused(_)
            | Self::RateLimited(_) => false,
        }
    }

    /// Whether a failed read is worth repeating: a network failure, a server
    /// error or a timeout, rather than an answer that rejected the request.
    pub(crate) fn is_transient(&self) -> bool {
        match self {
            Self::Transport(error) => !rejected_by_http_status(error),
            Self::Rpc { .. } | Self::RateLimited(_) | Self::Timeout(_) => true,
            Self::InvalidCredential(_)
            | Self::Authentication
            | Self::Refused(_)
            | Self::Unresolved(_)
            | Self::SchemaDrift { .. } => false,
        }
    }
}

/// An HTTP 4xx answer other than 408 rejected the request outright.
fn rejected_by_http_status(error: &reqwest::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.is_client_error() && status.as_u16() != 408)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io::{Read, Write};

    /// A real `reqwest` error for an HTTP answer with `status`, served by a
    /// one-shot local listener.
    pub(crate) fn http_status_error(status: u16) -> reqwest::Error {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("fixture connection");
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 {status} Fixture\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
        });
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .expect("fixture client")
            .get(format!("http://{address}/"))
            .send()
            .expect("fixture response");
        server.join().expect("fixture server");
        response
            .error_for_status()
            .expect_err("fixture status is an error")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::http_status_error;
    use super::*;

    #[test]
    fn http_client_errors_are_rejections_but_server_errors_are_not() {
        assert!(!Error::Transport(http_status_error(400)).may_have_committed());
        assert!(!Error::Transport(http_status_error(404)).may_have_committed());
        assert!(Error::Transport(http_status_error(408)).may_have_committed());
        assert!(Error::Transport(http_status_error(500)).may_have_committed());
        assert!(Error::Timeout("slow".into()).may_have_committed());
        assert!(!Error::RateLimited("quota".into()).may_have_committed());
        assert!(!Error::Transport(http_status_error(400)).is_transient());
        assert!(Error::Transport(http_status_error(408)).is_transient());
        assert!(Error::Transport(http_status_error(503)).is_transient());
        assert!(!Error::drift("listing", "changed").is_transient());
    }
}
