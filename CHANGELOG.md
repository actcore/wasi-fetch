# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-06-13

### Changed

- Rewrote the request/response internals on top of `wasip3::http_compat`:
  response bodies use wasip3's `IncomingBody` (inline stream reads) and request
  bodies stream via `BodyWriter` with structured concurrency. wasi-fetch no
  longer spawns a background task (`wit_bindgen::spawn`), so it shares the host
  component's single async runtime instead of coupling to a specific
  `wit-bindgen` version — the version-alignment hazard described in 0.1.3 is
  gone.
- Moved to the WASI 0.3 (final) toolchain via `wasip3` 0.7.0 (`wasi:http@0.3.0`).
  Components now require a host implementing `wasi:http@0.3.0` (e.g. Wasmtime
  46+); they will not instantiate on hosts shipping the `wasi:http@0.3.0-rc`
  interface (e.g. Wasmtime 45).
- The public API (`Client`, `RequestBuilder`, `Body`, `Error`) is unchanged.

### Removed

- Dropped the direct `wit-bindgen` and `flume` dependencies.

## [0.1.3] - 2026-04-19

### Changed

- Upgraded `wit-bindgen` from 0.56 to 0.57 and `wasip3` from 0.5 to 0.6. Consumers that use `wasi-fetch` alongside their own `wit_bindgen::generate!` must align on `wit-bindgen = "0.57"`; mismatched versions silently hang outbound POST requests because the generated `wasi:http` body-stream bindings diverge.

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
