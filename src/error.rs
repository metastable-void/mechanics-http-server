//! Error types for the HTTP/3 server surface.

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by `mechanics-http-server`.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The caller supplied TLS material rustls could not use.
    #[error("invalid TLS material: {0}")]
    InvalidTls(String),
    /// The QUIC endpoint could not bind its UDP socket.
    #[error("QUIC endpoint bind failed: {0}")]
    BindFailed(String),
    /// An internal QUIC, HTTP/3, tower, or task error occurred.
    #[error("internal: {0}")]
    Internal(String),
}
