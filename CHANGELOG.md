# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-04-16

### Added

- `between_bytes_timeout()` builder method for controlling max idle time between HTTP body frames — useful for reading SSE/streaming responses without hanging on keep-alive connections

### Changed

- Upgraded `wit-bindgen` from 0.55 to 0.56

## [0.1.1] - 2026-04-14

### Changed

- Upgrade wit-bindgen to 0.55 and wasip3 to 0.5

## [0.1.0] - 2026-03-21

Ergonomic HTTP client for WebAssembly components. Wraps wasip3 HTTP behind a reqwest-inspired API using standard `http` crate types.

### Added

- `Client` with builder pattern: `get()`, `post()`, `put()`, `delete()`, `patch()`, `head()`, `query()`, `request()`
- `RequestBuilder` with `.header()`, `.headers()`, `.body()`, `.json()`, `.timeout()`, `.redirect_limit()`
- Streaming `Body` type with `chunk()`, `bytes()`, `text()`, `json()` async methods
- `http_body::Body` trait implementation with demand-driven backpressure via `flume::bounded(1)`
- Redirect handling (default 10 hops, configurable, 303→GET conversion)
- Low-level `wasi_fetch::send(http::Request<Bytes>)` for direct use
- CI pipeline, release workflow for crates.io, pre-commit hooks

[0.1.2]: https://github.com/actcore/wasi-fetch/compare/0.1.1..0.1.2
[0.1.1]: https://github.com/actcore/wasi-fetch/compare/0.1.0..0.1.1
[0.1.0]: https://github.com/actcore/wasi-fetch/tree/0.1.0
