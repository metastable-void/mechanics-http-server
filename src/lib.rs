//! Opportunistic HTTP/3 server support for mechanics-family
//! services.
//!
//! This crate provides two additive building blocks for binaries
//! that already own their HTTP/1.1 and HTTP/2 TCP+TLS listener:
//! an opt-in HTTP/3 QUIC listener and a tower middleware layer
//! that emits `Alt-Svc` on the existing TCP+TLS path.
//!
//! Activation is controlled by [`Http3ServerConfig::bind_h3`].
//! When it is `None`, [`Http3Server::start`] returns an inert
//! handle immediately: no UDP socket is opened and no
//! [`quinn::Endpoint`] is constructed. When it is `Some`, this
//! crate binds that UDP address and routes HTTP/3 requests into
//! the supplied tower service.
//!
//! TLS material is supplied by the caller as
//! [`rustls::pki_types::CertificateDer`] and
//! [`rustls::pki_types::PrivateKeyDer`]. The QUIC-side TLS
//! configuration uses the `aws-lc-rs` rustls provider and ALPN
//! `h3` only. This server crate deliberately has no
//! `webpki-roots` dependency and no `ring` dependency.
//!
//! HTTP/1.1, HTTP/2 over TCP+TLS, and cleartext HTTP/1.1 remain
//! the consumer binary's responsibility. This crate does not
//! construct or alter those listeners.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

mod alt_svc;
pub mod axum_compat;
mod error;
mod server;
mod zero_rtt;

pub use alt_svc::{AltSvcLayer, AltSvcService, alt_svc_layer};
pub use error::{Error, Result};
pub use server::{Http3Handle, Http3Server, Http3ServerConfig};
pub use zero_rtt::{default_zero_rtt_methods, is_zero_rtt_safe};

#[cfg(test)]
mod tests;
