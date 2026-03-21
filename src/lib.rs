//! # wasi-fetch
//!
//! Ergonomic HTTP client for WebAssembly components.
//! Wraps the low-level wasip3 HTTP types behind a reqwest-inspired API,
//! using standard `http::Request`/`http::Response` types.
//!
//! ```ignore
//! use wasi_fetch::Client;
//!
//! let resp = Client::new().get("https://example.com").send().await?;
//! let body = resp.into_body().text().await?;
//! ```

mod body;
mod error;
mod request;

pub use body::Body;
pub use error::Error;
pub use request::RequestBuilder;

/// Stateless HTTP client.
pub struct Client;

impl Client {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::GET, url)
    }

    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::POST, url)
    }

    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::PUT, url)
    }

    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::DELETE, url)
    }

    pub fn patch(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::PATCH, url)
    }

    pub fn head(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(http::Method::HEAD, url)
    }

    pub fn request(&self, method: http::Method, url: &str) -> RequestBuilder {
        RequestBuilder::new(method, url)
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

/// Send an `http::Request<Vec<u8>>` via wasip3 and return `http::Response<Body>`.
///
/// Low-level function. Prefer `Client` builder for ergonomic usage.
pub async fn send(request: http::Request<Vec<u8>>) -> Result<http::Response<Body>, Error> {
    request::send_raw(request, None).await
}
