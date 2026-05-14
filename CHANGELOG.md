# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3] - 2026-05-14

### Added
- Added `H3RequestBody`, the streaming HTTP/3 request-body type
  passed into services by `Http3Server::start`.
- Added `H3RequestBodyError`, the error type emitted while
  receiving HTTP/3 request-body DATA frames or trailers.

### Changed
- Breaking: `Http3Server::start` now accepts services shaped as
  `Service<Request<H3RequestBody>, Response = Response<RespBody>>`
  where `RespBody: http_body::Body<Data = Bytes>`, so request and
  response bodies stream instead of using the round-01
  `Request<()> -> Response<Bytes>` shape.
- `axum_compat::router_into_h3_service` now forwards axum request
  and response bodies as streaming `http_body::Body` values.

### Removed
- Removed `DEFAULT_H3_RESPONSE_BODY_LIMIT_BYTES` and
  `router_into_h3_service_with_response_limit`; the HTTP/3 axum
  adapter no longer buffers response bodies behind a cap.
- Removed `AxumCompatError`; the axum adapter no longer performs
  fallible response-body collection.

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
