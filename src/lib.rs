//! Placeholder release for `mechanics-http-server` — the
//! server-side counterpart to
//! [`mechanics-http-client`](https://crates.io/crates/mechanics-http-client).
//!
//! Substantive content will land in a follow-up release as
//! part of the Philharmonic workspace's D22 server-side
//! dispatch. The goal is to provide an opportunistic HTTP/3
//! (QUIC) listener with `Alt-Svc` emission, sitting alongside
//! a traditional `hyper`-driven HTTP/1.1 + HTTP/2 listener
//! over TCP+TLS. Same TLS posture as `mechanics-http-client`:
//! `aws-lc-rs` crypto provider, `webpki-roots` Mozilla CA
//! bundle, no native trust, no `ring`.
//!
//! The crate is owned by the mechanics family and does not
//! depend on any philharmonic-family crate.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
