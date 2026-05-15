# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.4] - 2026-05-15

### Fixed
- `H3RequestBody` now treats DATA EOF as request-body
  completion and does not wait for optional trailers. The
  prior `recv_trailers().await` path kept the body future
  open after the final DATA frame so trailer-aware
  consumers could observe trailers; in practice, none of the
  workspace's services (connector-router, connector-service,
  api-server forwarder) consume request trailers, and an h3
  stack that does not resolve the "no trailers" phase
  promptly was holding complete POST bodies open
  indefinitely — exactly the pattern observed for
  `endpoint("llm")` POST calls reaching H3 then timing out
  at the mechanics 300 s outer timeout without ever reaching
  the upstream connector-service dial. The `ReadingTrailers`
  / `TrailersFuture` states and the `RecvTrailersFuture`
  alias are removed; the public `H3RequestBody` /
  `H3RequestBodyError` API surface is unchanged.
- QUIC server transport config now sets
  `keep_alive_interval = 15s` and `max_idle_timeout = 120s`.
  Without these, idle h3 connections silently die at
  NAT / stateful-firewall eviction (typically 30-60s on
  consumer-grade infra, longer on cloud), and the next
  request on a cached client-side h3 connection surfaces as
  a stream-level cancel with no useful detail. Server-side
  keep-alives ensure the connection's liveness probes find
  a willing peer; the matching `mhc 0.2.4` client-side
  setting drives the heartbeats. (Note: mhc 0.2.4 dropped
  its per-origin H3 sender cache, so client-side reuse
  across requests is no longer the dominant case;
  keep-alives now matter mostly for streaming responses
  that span the 30–60 s NAT-state TTL window.)

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
