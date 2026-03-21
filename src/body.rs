use bytes::{Bytes, BytesMut};
use http_body::Frame;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use wasip3::http::types::ErrorCode;

use crate::Error;

type WasiStream = wasip3::wit_bindgen::StreamReader<u8>;

const CHUNK_SIZE: usize = 16384;

fn new_trailers() -> wasip3::wit_bindgen::FutureReader<
    Result<Option<wasip3::http::types::Fields>, ErrorCode>,
> {
    wasip3::wit_future::new::<Result<Option<wasip3::http::types::Fields>, ErrorCode>>(|| Ok(None)).1
}

/// Shared waker slot — background reader wakes poll_frame after each send.
type SharedWaker = Arc<Mutex<Option<Waker>>>;

/// Streaming HTTP response body.
///
/// Implements [`http_body::Body`] for ecosystem compatibility.
/// Also provides convenience async methods: `bytes()`, `text()`, `json()`, `chunk()`.
///
/// **Important:** Use either the async methods OR `poll_frame`, not both.
pub struct Body {
    inner: BodyInner,
    _trailers:
        wasip3::wit_bindgen::FutureReader<Result<Option<wasip3::http::types::Fields>, ErrorCode>>,
}

enum BodyInner {
    /// Direct async stream with reusable buffers.
    Stream {
        stream: WasiStream,
        buf: Vec<u8>,
        acc: BytesMut,
    },
    /// Channel-based for poll_frame.
    Channel {
        rx: flume::Receiver<Option<Bytes>>,
        waker: SharedWaker,
    },
    /// Pre-buffered data.
    Buffered(Option<Bytes>),
    /// Fully consumed.
    Done,
}

impl Body {
    pub(crate) fn from_wasi(
        stream: WasiStream,
        trailers: wasip3::wit_bindgen::FutureReader<
            Result<Option<wasip3::http::types::Fields>, ErrorCode>,
        >,
    ) -> Self {
        Self {
            inner: BodyInner::Stream {
                stream,
                buf: Vec::with_capacity(CHUNK_SIZE),
                acc: BytesMut::with_capacity(CHUNK_SIZE * 2),
            },
            _trailers: trailers,
        }
    }

    /// Create an empty body.
    pub fn empty() -> Self {
        Self {
            inner: BodyInner::Done,
            _trailers: new_trailers(),
        }
    }

    /// Create a body from bytes.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let inner = if data.is_empty() {
            BodyInner::Done
        } else {
            BodyInner::Buffered(Some(Bytes::from(data)))
        };
        Self {
            inner,
            _trailers: new_trailers(),
        }
    }

    /// Read the next chunk from the body stream.
    ///
    /// Returns `None` when the body is fully consumed.
    pub async fn chunk(&mut self) -> Option<Bytes> {
        match &mut self.inner {
            BodyInner::Stream { stream, buf, acc } => {
                let read_buf = std::mem::take(buf);
                let (result, mut chunk) = stream.read(read_buf).await;
                match result {
                    wasip3::wit_bindgen::StreamResult::Complete(_) if !chunk.is_empty() => {
                        acc.extend_from_slice(&chunk);
                        chunk.clear();
                        *buf = chunk;
                        Some(acc.split().freeze())
                    }
                    _ => {
                        self.inner = BodyInner::Done;
                        None
                    }
                }
            }
            BodyInner::Buffered(data) => {
                let bytes = data.take();
                self.inner = BodyInner::Done;
                bytes
            }
            _ => None,
        }
    }

    /// Consume the entire body as bytes.
    pub async fn bytes(mut self) -> Bytes {
        // Optimized path for Stream: read directly into acc without split overhead
        if let BodyInner::Stream {
            mut stream,
            mut buf,
            mut acc,
        } = self.inner
        {
            self.inner = BodyInner::Done;
            loop {
                let (result, mut chunk) = stream.read(buf).await;
                match result {
                    wasip3::wit_bindgen::StreamResult::Complete(_) if !chunk.is_empty() => {
                        acc.extend_from_slice(&chunk);
                        chunk.clear();
                        buf = chunk;
                    }
                    _ => break,
                }
            }
            return acc.freeze();
        }

        // Buffered
        if let BodyInner::Buffered(data) = &mut self.inner {
            return data.take().unwrap_or_default();
        }

        Bytes::new()
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

    /// Move stream into a channel-based reader for poll_frame.
    fn ensure_channel(&mut self) {
        if !matches!(&self.inner, BodyInner::Stream { .. }) {
            return;
        }

        let old = std::mem::replace(&mut self.inner, BodyInner::Done);
        let BodyInner::Stream { mut stream, .. } = old else {
            unreachable!()
        };

        let waker: SharedWaker = Arc::new(Mutex::new(None));
        let waker_clone = waker.clone();

        // bounded(1) = backpressure: reader blocks until consumer drains
        let (tx, rx) = flume::bounded::<Option<Bytes>>(1);
        self.inner = BodyInner::Channel {
            rx,
            waker: waker.clone(),
        };

        wit_bindgen::spawn(async move {
            let mut buf = Vec::with_capacity(CHUNK_SIZE);
            let mut acc = BytesMut::with_capacity(CHUNK_SIZE * 2);
            loop {
                let (result, mut chunk) = stream.read(buf).await;
                match result {
                    wasip3::wit_bindgen::StreamResult::Complete(_) if !chunk.is_empty() => {
                        acc.extend_from_slice(&chunk);
                        chunk.clear();
                        buf = chunk;
                        if tx.send_async(Some(acc.split().freeze())).await.is_err() {
                            break;
                        }
                        // Wake the poll_frame consumer
                        if let Some(w) = waker_clone.lock().unwrap().take() {
                            w.wake();
                        }
                    }
                    _ => {
                        let _ = tx.send_async(None).await;
                        if let Some(w) = waker_clone.lock().unwrap().take() {
                            w.wake();
                        }
                        break;
                    }
                }
            }
        });
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // Handle Buffered variant directly
        if let BodyInner::Buffered(data) = &mut self.inner {
            let bytes = data.take();
            self.inner = BodyInner::Done;
            return match bytes {
                Some(b) => Poll::Ready(Some(Ok(Frame::data(b)))),
                None => Poll::Ready(None),
            };
        }

        // Ensure we have a channel
        self.ensure_channel();

        let BodyInner::Channel { rx, waker } = &mut self.inner else {
            return Poll::Ready(None);
        };

        match rx.try_recv() {
            Ok(Some(chunk)) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
            Ok(None) => {
                self.inner = BodyInner::Done;
                Poll::Ready(None)
            }
            Err(flume::TryRecvError::Empty) => {
                // Save waker — background reader will wake us after next send
                *waker.lock().unwrap() = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(flume::TryRecvError::Disconnected) => {
                self.inner = BodyInner::Done;
                Poll::Ready(None)
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self.inner, BodyInner::Done)
    }
}
