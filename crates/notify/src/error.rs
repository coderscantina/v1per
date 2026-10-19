use std::time::Duration;

/// Why a send failed. Messages never contain secrets or webhook URLs.
#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("invalid channel config: {0}")]
    Config(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("provider type {0} is not supported yet")]
    Unsupported(String),
    #[error("provider answered HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("mail server error: {message}")]
    Smtp { message: String, permanent: bool },
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("timed out after {0:?}")]
    Timeout(Duration),
}

impl SendError {
    /// Whether the same send may succeed later without changing the channel config.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Config(_) | Self::Template(_) | Self::Unsupported(_) => false,
            Self::Http { status, .. } => matches!(status, 408 | 425 | 429 | 500..),
            Self::Smtp { permanent, .. } => !permanent,
            Self::Connection(_) | Self::Timeout(_) => true,
        }
    }
}
