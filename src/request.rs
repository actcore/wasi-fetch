use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use wasip3::http::types::{
    ErrorCode, Fields, Request, RequestOptions, Response as WasiResponse, Scheme,
};

use crate::{Body, Error};

const DEFAULT_REDIRECT_LIMIT: u8 = 10;

/// Builder for an HTTP request.
pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
    timeout_ms: Option<u64>,
    redirect_limit: u8,
}

impl RequestBuilder {
    pub(crate) fn new(method: Method, url: &str) -> Self {
        Self {
            method,
            url: url.to_string(),
            headers: HeaderMap::new(),
            body: None,
            timeout_ms: None,
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

    /// Set raw body bytes.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Set JSON body. Automatically sets `Content-Type: application/json`.
    pub fn json<T: serde::Serialize>(mut self, value: &T) -> Self {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.body = Some(bytes);
            self.headers.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        self
    }

    /// Set request timeout in milliseconds.
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Set maximum number of redirects to follow. Default is 10. Set to 0 to disable.
    pub fn redirect_limit(mut self, max: u8) -> Self {
        self.redirect_limit = max;
        self
    }

    /// Send the request and return an `http::Response<Body>`.
    pub async fn send(self) -> Result<http::Response<Body>, Error> {
        let timeout_ms = self.timeout_ms;
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
                Vec::new()
            } else {
                original_body.clone().unwrap_or_default()
            };

            let request = builder
                .body(req_body)
                .map_err(|e| Error::Url(format!("Failed to build request: {e}")))?;

            let response = send_raw(request, timeout_ms).await?;

            let status = response.status();

            if redirect_limit > 0 && status.is_redirection() {
                redirects += 1;
                if redirects > redirect_limit {
                    return Err(Error::Transport("Too many redirects".to_string()));
                }

                if let Some(location) = response.headers().get(http::header::LOCATION) {
                    let location_str = location.to_str().map_err(|e| {
                        Error::Transport(format!("Invalid Location header: {e}"))
                    })?;

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

/// Send an `http::Request<Vec<u8>>` over wasip3 HTTP transport (no redirect handling).
pub(crate) async fn send_raw(
    request: http::Request<Vec<u8>>,
    timeout_ms: Option<u64>,
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

    // Body stream
    let body_stream = if body.is_empty() {
        None
    } else {
        let (mut writer, reader) = wasip3::wit_stream::new::<u8>();
        wit_bindgen::spawn(async move {
            writer.write_all(body).await;
        });
        Some(reader)
    };

    // Trailers (none)
    let (_, trailers_reader) =
        wasip3::wit_future::new::<Result<Option<Fields>, ErrorCode>>(|| Ok(None));

    // Timeout
    let opts = timeout_ms.map(|ms| {
        let ns = ms * 1_000_000;
        let opts = RequestOptions::new();
        let _ = opts.set_connect_timeout(Some(ns));
        let _ = opts.set_first_byte_timeout(Some(ns));
        opts
    });

    // Build WASI request
    let (wasi_request, _) = Request::new(fields, body_stream, trailers_reader, opts);
    let _ = wasi_request.set_method(&to_wasi_method(&parts.method));
    let _ = wasi_request.set_scheme(Some(&scheme));

    if let Some(authority) = uri.authority() {
        let _ = wasi_request.set_authority(Some(authority.as_str()));
    }

    let _ = wasi_request.set_path_with_query(uri.path_and_query().map(|pq| pq.as_str()));

    // Send
    let wasi_response = wasip3::http::client::send(wasi_request)
        .await
        .map_err(|e| Error::Transport(format!("{e:?}")))?;

    // Read response headers
    let resp_fields = wasi_response.get_headers();
    let mut resp_headers = HeaderMap::new();
    for (name, value) in resp_fields.copy_all() {
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_bytes(&value),
        ) {
            resp_headers.append(hn, hv);
        }
    }

    let status = wasi_response.get_status_code();

    // Consume body as streaming
    let (_, result_reader) = wasip3::wit_future::new::<Result<(), ErrorCode>>(|| Ok(()));
    let (body_reader, trailers) = WasiResponse::consume_body(wasi_response, result_reader);

    let body = Body::from_wasi(body_reader, trailers);

    // Build http::Response
    let mut builder = http::Response::builder().status(status);
    if let Some(headers) = builder.headers_mut() {
        *headers = resp_headers;
    }

    builder
        .body(body)
        .map_err(|e| Error::Transport(format!("Failed to build response: {e}")))
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
