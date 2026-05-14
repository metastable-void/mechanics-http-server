# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-05-14

### Added
- Added `axum_compat::router_into_h3_service` so axum-based
  services can be routed through `Http3Server::start`.

## [0.1.1] - 2026-05-14

### Changed
- Internal Cargo.toml audit: `default-features = false` set on
  direct dependencies with explicit feature lists for what the
  crate actually uses. No behaviour change. (D24)

## [0.1.0] - 2026-05-13

### Added

- Ship the first substantive D22 server library release from ROADMAP
  §3.I: an opt-in HTTP/3 QUIC listener, Alt-Svc tower middleware,
  and conservative 0-RTT method policy for mechanics-family servers.
