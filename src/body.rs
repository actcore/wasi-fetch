use bytes::{Bytes, BytesMut};
use http_body::{Body as HttpBody, Frame};
use std::pin::Pin;
use std::task::{Context, Poll};
use wasip3::http_compat::IncomingResponseBody;

use crate::Error;

/// Streaming HTTP response body.
///
/// Implements [`http_body::Body`] for ecosystem compatibility.
/// Also provides convenience async methods: `bytes()`, `text()`, `json()`, `chunk()`.
///
/// The streaming path wraps wasip3's [`IncomingResponseBody`], which reads the
/// underlying `wasi:http` body stream inline (no background task), so the body
/// is driven by whatever executor polls it.
pub struct Body {
    inner: BodyInner,
}

enum BodyInner {
    /// Streaming response body backed by the `wasi:http` stream.
    Incoming(IncomingResponseBody),
    /// Pre-buffered data (request bodies, `from_bytes`).
    Buffered(Option<Bytes>),
    /// Fully consumed / empty.
    Done,
}

impl Body {
    /// Wrap a wasip3 incoming response body.
    pub(crate) fn from_incoming(incoming: IncomingResponseBody) -> Self {
        Self {
            inner: BodyInner::Incoming(incoming),
        }
    }

    /// Create an empty body.
    pub fn empty() -> Self {
        Self {
            inner: BodyInner::Done,
        }
    }

    /// Create a body from bytes.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let inner = if data.is_empty() {
            BodyInner::Done
        } else {
            BodyInner::Buffered(Some(Bytes::from(data)))
        };
        Self { inner }
    }

    /// Read the next data chunk from the body stream.
    ///
    /// Returns `None` when the body is fully consumed. Trailer frames are
    /// skipped (use `poll_frame` if you need trailers).
    pub async fn chunk(&mut self) -> Option<Bytes> {
        match &mut self.inner {
            BodyInner::Incoming(incoming) => loop {
                let frame =
                    std::future::poll_fn(|cx| Pin::new(&mut *incoming).poll_frame(cx)).await;
                match frame {
                    Some(Ok(frame)) => match frame.into_data() {
                        Ok(data) => return Some(data),
                        // Trailers frame — skip and keep reading.
                        Err(_) => continue,
                    },
                    Some(Err(_)) | None => {
                        self.inner = BodyInner::Done;
                        return None;
                    }
                }
            },
            BodyInner::Buffered(data) => {
                let bytes = data.take();
                self.inner = BodyInner::Done;
                bytes
            }
            BodyInner::Done => None,
        }
    }

    /// Consume the entire body as bytes.
    pub async fn bytes(mut self) -> Bytes {
        match &mut self.inner {
            BodyInner::Incoming(_) => {
                let mut acc = BytesMut::new();
                while let Some(chunk) = self.chunk().await {
                    acc.extend_from_slice(&chunk);
                }
                acc.freeze()
            }
            BodyInner::Buffered(data) => data.take().unwrap_or_default(),
            BodyInner::Done => Bytes::new(),
        }
    }

    /// Consume the entire body as a UTF-8 string.
    pub async fn text(self) -> Result<String, Error> {
        let body = self.bytes().await;
        String::from_utf8(body.to_vec()).map_err(Error::Utf8)
    }

    /// Consume the entire body and deserialize as JSON.
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, Error> {
        let body = self.bytes().await;
        serde_json::from_slice(&body).map_err(Error::Json)
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match &mut self.inner {
            BodyInner::Incoming(incoming) => match Pin::new(incoming).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(frame))),
                Poll::Ready(Some(Err(e))) => {
                    Poll::Ready(Some(Err(Error::Transport(format!("{e:?}")))))
                }
                Poll::Ready(None) => {
                    self.inner = BodyInner::Done;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            },
            BodyInner::Buffered(data) => {
                let bytes = data.take();
                self.inner = BodyInner::Done;
                match bytes {
                    Some(b) => Poll::Ready(Some(Ok(Frame::data(b)))),
                    None => Poll::Ready(None),
                }
            }
            BodyInner::Done => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self.inner, BodyInner::Done)
    }
}
