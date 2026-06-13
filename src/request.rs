use bytes::Bytes;
use futures::join;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use wasip3::http::types::{ErrorCode, Fields, Request, RequestOptions, Scheme};
use wasip3::http_compat::{BodyWriter, http_from_wasi_response};

use crate::{Body, Error};

const DEFAULT_REDIRECT_LIMIT: u8 = 10;

/// Builder for an HTTP request.
pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: HeaderMap,
    body: Option<Bytes>,
    timeout: Option<std::time::Duration>,
    between_bytes_timeout: Option<std::time::Duration>,
    redirect_limit: u8,
}

impl RequestBuilder {
    pub(crate) fn new(method: Method, url: &str) -> Self {
        Self {
            method,
            url: url.to_string(),
            headers: HeaderMap::new(),
            body: None,
            timeout: None,
            between_bytes_timeout: None,
            redirect_limit: DEFAULT_REDIRECT_LIMIT,
        }
    }

    /// Add a header.
    pub fn header(
        mut self,
        name: impl TryInto<HeaderName>,
        value: impl TryInto<HeaderValue>,
    ) -> Self {
        if let (Ok(name), Ok(value)) = (name.try_into(), value.try_into()) {
            self.headers.insert(name, value);
        }
        self
    }

    /// Replace all headers.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    /// Set request body.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set JSON body. Automatically sets `Content-Type: application/json`.
    pub fn json<T: serde::Serialize>(mut self, value: &T) -> Self {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.body = Some(Bytes::from(bytes));
            self.headers.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        self
    }

    /// Set request timeout (applies to connect and first-byte).
    pub fn timeout(mut self, duration: std::time::Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Set between-bytes timeout (max idle time between body frames).
    ///
    /// Useful for SSE/streaming responses where the server may keep the
    /// connection open indefinitely after sending data.
    pub fn between_bytes_timeout(mut self, duration: std::time::Duration) -> Self {
        self.between_bytes_timeout = Some(duration);
        self
    }

    /// Set maximum number of redirects to follow. Default is 10. Set to 0 to disable.
    pub fn redirect_limit(mut self, max: u8) -> Self {
        self.redirect_limit = max;
        self
    }

    /// Send the request and return an `http::Response<Body>`.
    pub async fn send(self) -> Result<http::Response<Body>, Error> {
        let timeout = self.timeout;
        let between_bytes_timeout = self.between_bytes_timeout;
        let redirect_limit = self.redirect_limit;
        let original_body = self.body.clone();

        let mut method = self.method;
        let mut current_url: Uri = self
            .url
            .parse()
            .map_err(|e| Error::Url(format!("Invalid URL '{}': {e}", self.url)))?;
        let headers = self.headers;
        let body = self.body;

        let mut redirects = 0u8;

        loop {
            let mut builder = http::Request::builder()
                .method(method.clone())
                .uri(&current_url);

            if let Some(h) = builder.headers_mut() {
                *h = headers.clone();
            }

            // Don't send body on redirected GET/HEAD
            let req_body = if redirects == 0 {
                body.clone().unwrap_or_default()
            } else if method == Method::GET || method == Method::HEAD {
                Bytes::new()
            } else {
                original_body.clone().unwrap_or_default()
            };

            let request = builder
                .body(req_body)
                .map_err(|e| Error::Url(format!("Failed to build request: {e}")))?;

            let response = send_raw(request, timeout, between_bytes_timeout).await?;

            let status = response.status();

            if redirect_limit > 0 && status.is_redirection() {
                redirects += 1;
                if redirects > redirect_limit {
                    return Err(Error::Transport("Too many redirects".to_string()));
                }

                if let Some(location) = response.headers().get(http::header::LOCATION) {
                    let location_str = location
                        .to_str()
                        .map_err(|e| Error::Transport(format!("Invalid Location header: {e}")))?;

                    current_url = resolve_redirect(&current_url, location_str)?;

                    // 303 See Other: change method to GET
                    if status == StatusCode::SEE_OTHER {
                        method = Method::GET;
                    }
                    continue;
                }
            }

            return Ok(response);
        }
    }
}

/// Resolve a redirect Location against the current URI.
fn resolve_redirect(base: &Uri, location: &str) -> Result<Uri, Error> {
    let base_url = url::Url::parse(&base.to_string())
        .map_err(|e| Error::Url(format!("Invalid base URL: {e}")))?;
    let resolved = base_url
        .join(location)
        .map_err(|e| Error::Url(format!("Invalid redirect URL: {e}")))?;
    resolved
        .as_str()
        .parse()
        .map_err(|e| Error::Url(format!("Invalid redirect URL: {e}")))
}

/// Send an `http::Request` over wasip3 HTTP transport (no redirect handling).
pub(crate) async fn send_raw(
    request: http::Request<Bytes>,
    timeout: Option<std::time::Duration>,
    between_bytes_timeout: Option<std::time::Duration>,
) -> Result<http::Response<Body>, Error> {
    let (parts, body) = request.into_parts();

    let uri: Uri = parts
        .uri
        .to_string()
        .parse()
        .map_err(|e| Error::Url(format!("Invalid URI: {e}")))?;

    let scheme = match uri.scheme_str() {
        Some("https") => Scheme::Https,
        Some("http") => Scheme::Http,
        Some(other) => return Err(Error::Url(format!("Unsupported scheme: {other}"))),
        None => return Err(Error::Url("Missing URL scheme".to_string())),
    };

    // Convert headers
    let header_list: Vec<(String, Vec<u8>)> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
        .collect();
    let fields = Fields::from_list(&header_list)
        .map_err(|e| Error::Transport(format!("Invalid headers: {e:?}")))?;

    // Timeout / between-bytes options
    let opts = if timeout.is_some() || between_bytes_timeout.is_some() {
        let opts = RequestOptions::new();
        if let Some(d) = timeout {
            let ns = d.as_nanos() as u64;
            let _ = opts.set_connect_timeout(Some(ns));
            let _ = opts.set_first_byte_timeout(Some(ns));
        }
        if let Some(d) = between_bytes_timeout {
            let _ = opts.set_between_bytes_timeout(Some(d.as_nanos() as u64));
        }
        Some(opts)
    } else {
        None
    };

    // Build the WASI request. A non-empty body is streamed concurrently with
    // `send` via a BodyWriter (structured concurrency — no detached task); an
    // empty body uses no body stream.
    let (body_writer, wasi_request) = if body.is_empty() {
        let (_, trailers) =
            wasip3::wit_future::new::<Result<Option<Fields>, ErrorCode>>(|| Ok(None));
        let (request, _) = Request::new(fields, None, trailers, opts);
        (None, request)
    } else {
        let (writer, body_reader, trailers_reader) = BodyWriter::new();
        let (request, _) = Request::new(fields, Some(body_reader), trailers_reader, opts);
        (Some(writer), request)
    };

    let _ = wasi_request.set_method(&to_wasi_method(&parts.method));
    let _ = wasi_request.set_scheme(Some(&scheme));
    if let Some(authority) = uri.authority() {
        let _ = wasi_request.set_authority(Some(authority.as_str()));
    }
    let _ = wasi_request.set_path_with_query(uri.path_and_query().map(|pq| pq.as_str()));

    // Send, writing the request body concurrently when present.
    let wasi_response = match body_writer {
        Some(writer) => {
            let mut req_body = Body::from_bytes(body.to_vec());
            let (response, _written) = join!(
                wasip3::http::client::send(wasi_request),
                writer.send_http_body(&mut req_body),
            );
            response
        }
        None => wasip3::http::client::send(wasi_request).await,
    }
    .map_err(|e| Error::Transport(format!("{e:?}")))?;

    // Map the WASI response (status, headers, streaming body) into http::Response.
    let (resp_parts, incoming) = http_from_wasi_response(wasi_response)
        .map_err(|e| Error::Transport(format!("{e:?}")))?
        .into_parts();

    Ok(http::Response::from_parts(
        resp_parts,
        Body::from_incoming(incoming),
    ))
}

fn to_wasi_method(m: &Method) -> wasip3::http::types::Method {
    use wasip3::http::types::Method as WM;
    match *m {
        Method::GET => WM::Get,
        Method::POST => WM::Post,
        Method::PUT => WM::Put,
        Method::DELETE => WM::Delete,
        Method::PATCH => WM::Patch,
        Method::HEAD => WM::Head,
        Method::OPTIONS => WM::Options,
        _ => WM::Other(m.to_string()),
    }
}
