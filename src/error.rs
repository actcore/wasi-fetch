/// Error type for wasi-fetch operations.
#[derive(Debug)]
pub enum Error {
    /// Invalid URL.
    Url(String),
    /// HTTP transport error (connection, DNS, TLS, etc.).
    Transport(String),
    /// Response body is not valid UTF-8.
    Utf8(std::string::FromUtf8Error),
    /// JSON deserialization failed.
    Json(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Url(msg) => write!(f, "URL error: {msg}"),
            Error::Transport(msg) => write!(f, "HTTP transport error: {msg}"),
            Error::Utf8(e) => write!(f, "UTF-8 error: {e}"),
            Error::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Utf8(e) => Some(e),
            Error::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::Utf8(e)
    }
}
