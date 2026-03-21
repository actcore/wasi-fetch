# wasi-fetch

Ergonomic HTTP client for WebAssembly components. Wraps the low-level wasip3 HTTP types behind a reqwest-inspired API, using standard `http::Request`/`http::Response` types.

## Usage

```toml
[dependencies]
wasi-fetch = "0.1"
```

```rust
use wasi_fetch::Client;

// GET
let resp = Client::new().get("https://example.com/api").send().await?;
let status = resp.status();
let body = String::from_utf8(resp.into_body())?;

// POST with JSON
let resp = Client::new()
    .post("https://example.com/api")
    .header("authorization", "Bearer token")
    .json(&serde_json::json!({"key": "value"}))
    .send()
    .await?;

// Low-level: send http::Request directly
let request = http::Request::get("https://example.com")
    .body(vec![])
    .unwrap();
let response = wasi_fetch::send(request).await?;
```

## Features

- **`http` crate types** — `http::Response<Vec<u8>>` return type, `http::HeaderMap`, `http::StatusCode`
- **Builder API** — `Client::new().get(url).header(...).json(...).timeout_ms(...).send().await`
- **Low-level `send()`** — pass an `http::Request<Vec<u8>>` directly
- **Timeout** — per-request connect + first-byte timeout

## Target

`wasm32-wasip2` with nightly Rust toolchain.

## License

MIT OR Apache-2.0
