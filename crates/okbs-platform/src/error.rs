//! Errors of platform backends.

/// Failure of a platform operation.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// Not available on this OS, desktop or session.
    #[error("not supported here: {0}")]
    Unsupported(&'static str),
    /// Access to a device or API was denied.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// An OS API returned an error code.
    #[error("{context} failed with OS error {code}")]
    Os {
        /// OS error code.
        code: i64,
        /// Operation that failed.
        context: String,
    },
    /// Anything else.
    #[error("{0}")]
    Other(String),
}
