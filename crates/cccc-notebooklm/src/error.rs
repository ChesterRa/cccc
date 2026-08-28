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
}
