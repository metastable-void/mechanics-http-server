# mechanics-http-server

Server-side counterpart to
[`mechanics-http-client`](https://crates.io/crates/mechanics-http-client):
opportunistic HTTP/3 (QUIC) listener with `Alt-Svc` header
emission, sitting alongside the traditional `hyper`-driven
HTTP/1.1 + HTTP/2 listener over TCP+TLS. Same TLS posture
across both transports: `aws-lc-rs` crypto provider,
`webpki-roots` Mozilla CA bundle (or operator-supplied
server cert/key), no `ring`, no OS-native trust.

Owned by the mechanics family. **Does not depend on any
`philharmonic-*` crate** — Philharmonic-family consumers
depend on this crate, never the reverse.

## Status

**v0.0.0 — name reservation.** Substantive content lands in
a follow-up release as part of the Philharmonic workspace's
D22 server-side dispatch. Tracking issue / design lives at
[`docs/ROADMAP.md` §3.I](https://github.com/metastable-void/philharmonic-workspace/blob/main/docs/ROADMAP.md)
in the workspace meta-repo.

## License

Dual-licensed under `Apache-2.0 OR MPL-2.0`.

## Contributing

Developed as part of the
[mechanics-rs](https://github.com/metastable-void/mechanics-rs)
family, under the
[philharmonic-workspace](https://github.com/metastable-void/philharmonic-workspace)
parent. Workspace-wide development conventions — git workflow,
script wrappers, Rust code rules, versioning, terminology —
live in the meta-repo, authoritatively in its
[`CONTRIBUTING.md`](https://github.com/metastable-void/philharmonic-workspace/blob/main/CONTRIBUTING.md).

SPDX-License-Identifier: Apache-2.0 OR MPL-2.0
