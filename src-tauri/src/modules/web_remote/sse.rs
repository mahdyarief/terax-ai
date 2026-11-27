//! Server-sent events: one response that stays open and pushes named events.
//!
//! The event id is always the replay sequence for a terminal, so a reconnect
//! with `Last-Event-ID` resumes exactly where the stream stopped.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE};
use hyper::Response;
use tokio::sync::mpsc;

use super::server::Body;

/// How often a quiet stream says it is still there.
pub const HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(25);
/// Events a viewer may fall behind before the stream is rebuilt.
pub const STREAM_BUFFER: usize = 64;

pub(crate) type Item = Result<Frame<Bytes>, Infallible>;

/// One SSE line set. `data` is a single line (JSON or base64), so nothing in
/// it can split an event.
pub fn event(id: Option<u64>, name: &str, data: &str) -> Bytes {
    let mut out = String::with_capacity(data.len() + name.len() + 24);
    if let Some(id) = id {
        out.push_str("id: ");
        out.push_str(&id.to_string());
        out.push('\n');
    }
    out.push_str("event: ");
    out.push_str(name);
    out.push_str("\ndata: ");
    out.push_str(data);
    out.push_str("\n\n");
    Bytes::from(out)
}

pub fn comment(text: &str) -> Bytes {
    Bytes::from(format!(": {text}\n\n"))
}

/// The response for a stream, with `rx` feeding it.
pub fn response(rx: mpsc::Receiver<Item>) -> Response<Body> {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    let body = BodyExt::boxed(StreamBody::new(stream));
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Proxies must not buffer an event stream.
    headers.insert("X-Accel-Buffering", HeaderValue::from_static("no"));
    response
}

/// Keeps the viewer count honest even when the connection is dropped without
/// the task noticing.
pub struct ViewerGuard(pub Arc<AtomicUsize>);

impl ViewerGuard {
    pub fn open(viewers: &Arc<AtomicUsize>) -> Self {
        viewers.fetch_add(1, Ordering::Relaxed);
        Self(viewers.clone())
    }
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The id a resumed stream asks to continue after.
pub fn last_event_id(headers: &hyper::header::HeaderMap) -> Option<u64> {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
}

pub(crate) fn send(tx: &mpsc::Sender<Item>, bytes: Bytes) {
    // A viewer that cannot keep up is disconnected, not queued forever; the
    // browser reconnects and resumes from its last event id.
    let _ = tx.try_send(Ok(Frame::data(bytes)));
}

/// Awaits one item so the stream task yields between bursts.
pub(crate) async fn reserve(tx: &mpsc::Sender<Item>) -> bool {
    tx.reserve().await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_carries_its_id_and_no_extra_lines() {
        let bytes = event(Some(42), "data", "aGVsbG8=");
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            "id: 42\nevent: data\ndata: aGVsbG8=\n\n"
        );
        let no_id = event(None, "meta", "{}");
        assert_eq!(
            std::str::from_utf8(&no_id).unwrap(),
            "event: meta\ndata: {}\n\n"
        );
    }

    #[test]
    fn the_viewer_count_returns_to_zero() {
        let viewers = Arc::new(AtomicUsize::new(0));
        {
            let _one = ViewerGuard::open(&viewers);
            let _two = ViewerGuard::open(&viewers);
            assert_eq!(viewers.load(Ordering::Relaxed), 2);
        }
        assert_eq!(viewers.load(Ordering::Relaxed), 0);
    }
}
