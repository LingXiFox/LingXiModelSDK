//! Incremental native streaming primitives.
//!
//! `EventStreamHandle` owns an in-flight HTTP response and decodes protocol
//! framing (SSE / NDJSON / Amazon Event Stream) on demand. Because the caller
//! pulls events one at a time, backpressure is consumer-driven: no bytes are
//! read from the socket until the next event is requested.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use crate::errors::NativeError;
use crate::eventstream::EventStream;
use crate::sse::{NdjsonParser, SseEvent, SseParser};

/// Maximum number of payload bytes a single stream may deliver.
pub const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
#[allow(dead_code)] // the variant names are used for diagnostics only
pub(crate) enum StreamDecoder {
    Sse(SseParser),
    Ndjson(NdjsonParser),
    AmazonEventStream(EventStream),
}

/// A live, incrementally decoded upstream response.
///
/// The handle is `Send` but not `Sync`: poll it from one task at a time. The
/// cancellation token is shared, so any thread may call
/// [`EventStreamHandle::cancel`]; dropping the handle aborts the request and
/// releases the connection.
pub struct EventStreamHandle {
    resp: reqwest::Response,
    decoder: StreamDecoder,
    cancel: Arc<AtomicBool>,
    idle_timeout: Duration,
    /// Wakes a blocked reader so `cancel` takes effect mid-read.
    notify: Arc<Notify>,
    status: u16,
    headers: HashMap<String, String>,
    pending: VecDeque<SseEvent>,
    total_bytes: usize,
    done: bool,
}

impl std::fmt::Debug for EventStreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStreamHandle")
            .field("status", &self.status)
            .field("cancelled", &self.is_cancelled())
            .field("done", &self.done)
            .field("bytes", &self.total_bytes)
            .finish_non_exhaustive()
    }
}

impl EventStreamHandle {
    pub(crate) fn new(
        resp: reqwest::Response,
        decoder: StreamDecoder,
        idle_timeout: Duration,
    ) -> Self {
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|value| (k.as_str().to_string(), value.to_string()))
            })
            .collect();
        Self {
            resp,
            decoder,
            cancel: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
            idle_timeout,
            status,
            headers,
            pending: VecDeque::new(),
            total_bytes: 0,
            done: false,
        }
    }

    /// Upstream HTTP status of the accepted streaming response.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Response headers captured when the stream was opened.
    pub fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Handle for waking blocked readers out of a parked socket read.
    pub fn cancellation_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.notify)
    }

    /// Shared cancellation token; clone it to abort from another thread.
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// Requests cancellation. The next `next_event` call fails immediately.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Payload bytes consumed so far.
    pub fn bytes_read(&self) -> usize {
        self.total_bytes
    }

    /// Reads the next decoded event, or `None` once the upstream body ends.
    pub async fn next_event(&mut self) -> Result<Option<SseEvent>, NativeError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            if self.done {
                return Ok(None);
            }
            if self.is_cancelled() {
                self.done = true;
                return Err(NativeError::Cancelled("stream cancelled by caller".into()));
            }
            // Park on the socket read but stay interruptible: `cancel` wakes
            // the waiter instead of leaving the caller stuck until the next
            // upstream byte arrives.
            let chunk = tokio::select! {
                biased;
                _ = self.notify.notified() => {
                    self.done = true;
                    return Err(NativeError::Cancelled("stream cancelled by caller".into()));
                }
                result = tokio::time::timeout(self.idle_timeout, self.resp.chunk()) => match result {
                    Ok(Ok(Some(chunk))) => chunk,
                    Ok(Ok(None)) => {
                        self.finish()?;
                        continue;
                    }
                    Ok(Err(e)) => {
                        self.done = true;
                        if self.is_cancelled() {
                            return Err(NativeError::Cancelled("stream cancelled by caller".into()));
                        }
                        return Err(NativeError::Transport(e.to_string()));
                    }
                    Err(_) => {
                        self.done = true;
                        return Err(NativeError::Timeout(format!(
                            "no stream data for {} ms",
                            self.idle_timeout.as_millis()
                        )));
                    }
                },
            };
            self.total_bytes += chunk.len();
            if self.total_bytes > MAX_STREAM_BYTES {
                self.done = true;
                return Err(NativeError::Transport(format!(
                    "stream exceeded {MAX_STREAM_BYTES} byte limit"
                )));
            }
            let events = match &mut self.decoder {
                StreamDecoder::Sse(parser) => parser.push(&chunk[..]),
                StreamDecoder::Ndjson(parser) => parser.push(&chunk[..]),
                StreamDecoder::AmazonEventStream(parser) => parser.push(&chunk)?,
            };
            self.pending.extend(events);
        }
    }

    fn finish(&mut self) -> Result<(), NativeError> {
        self.done = true;
        match &mut self.decoder {
            StreamDecoder::Sse(parser) => {
                self.pending.extend(parser.finish());
            }
            StreamDecoder::Ndjson(parser) => {
                self.pending.extend(parser.finish());
            }
            StreamDecoder::AmazonEventStream(parser) => parser.finish()?,
        }
        Ok(())
    }

    /// Drains the stream into memory. Kept for compatibility with callers that
    /// cannot consume incrementally; prefer `next_event` for latency.
    pub async fn collect(&mut self) -> Result<Vec<SseEvent>, NativeError> {
        let mut events = Vec::new();
        while let Some(event) = self.next_event().await? {
            events.push(event);
        }
        Ok(events)
    }
}
