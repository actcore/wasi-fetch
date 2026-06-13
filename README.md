# wasi-fetch

Ergonomic HTTP client for WebAssembly components. Wraps the low-level wasip3 HTTP types behind a reqwest-inspired API, using standard `http::Request`/`http::Response` types.

## Usage

```toml
[dependencies]
wasi-fetch = "0.2"
```

```rust
use wasi_fetch::Client;

// GET
let resp = Client::new().get("https://example.com/api").send().await?;
let status = resp.status();
let body = resp.into_body().text().await?; // or .bytes().await / .json::<T>().await

// POST with JSON
let resp = Client::new()
    .post("https://example.com/api")
    .header("authorization", "Bearer token")
    .json(&serde_json::json!({"key": "value"}))
    .send()
    .await?;

// Low-level: send an http::Request directly
let request = http::Request::get("https://example.com")
    .body(bytes::Bytes::new())
    .unwrap();
let response = wasi_fetch::send(request).await?;
```

## Features

- **`http` crate types** — returns `http::Response<Body>`, uses `http::HeaderMap`, `http::StatusCode`
- **Builder API** — `Client::new().get(url).header(...).json(...).timeout(...).send().await`
- **Streaming `Body`** — `.text()`, `.bytes()`, `.json::<T>()`, and `.chunk()` for incremental reads; also implements `http_body::Body`
- **Low-level `send()`** — pass an `http::Request<bytes::Bytes>` directly
- **Timeouts** — per-request connect + first-byte, plus a between-bytes timeout for SSE/streaming responses
- **Redirects** — followed by default, with a configurable limit

## Implementation

wasi-fetch is a thin adapter over [`wasip3::http_compat`](https://docs.rs/wasip3): response bodies stream through wasip3's `IncomingBody` and request bodies through its `BodyWriter`, both driven inline by the caller's executor — there is no background task, so wasi-fetch shares the host component's single async runtime rather than pinning a particular `wit-bindgen` version.

## Compatibility

Built on `wasip3` 0.7 (`wasi:http@0.3.0`, final). Components using wasi-fetch require a host that implements `wasi:http@0.3.0` (e.g. Wasmtime 46+); they will **not** instantiate on hosts that still ship the `wasi:http@0.3.0-rc` interface (e.g. Wasmtime 45).

## Target

`wasm32-wasip2` with nightly Rust toolchain.

## License

MIT OR Apache-2.0
