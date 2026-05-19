//! Transparent **tee adapters** for the agent's stdio, feeding the live
//! wire console.
//!
//! `sacp` owns the JSON-RPC transport (the child's stdin/stdout), so the raw
//! bytes are invisible to the UI — exactly what you need to see when a
//! custom ACP command "just hangs on spawning". These zero-behaviour
//! wrappers sit *between* the child handles and `sacp`: every chunk written
//! to the agent or read from it is delegated through unchanged **and** a
//! length-bounded UTF-8-lossy copy is sent to the reducer as
//! [`AcpEvent::Wire`]. Best-effort: a full/closed channel just drops the
//! copy — the protocol stream is never affected.

use std::pin::Pin;
use std::sync::mpsc::Sender;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::events::{AcpEvent, WireDir};

/// Longest chunk copied to the console in one event (the protocol stream is
/// untouched; only the *observed* copy is clipped so one big frame cannot
/// flood the channel).
const MAX_CHUNK: usize = 4096;

fn emit(tx: &Sender<AcpEvent>, dir: WireDir, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let slice = &bytes[..bytes.len().min(MAX_CHUNK)];
    let mut text = String::from_utf8_lossy(slice).into_owned();
    if bytes.len() > MAX_CHUNK {
        text.push_str("…⟨truncated⟩");
    }
    let _ = tx.send(AcpEvent::Wire { dir, text });
}

/// Wraps the agent's stdin: tees every write as [`WireDir::ToAgent`].
pub struct TeeWrite<W> {
    inner: W,
    tx: Sender<AcpEvent>,
}

impl<W> TeeWrite<W> {
    /// Wrap `inner`, copying writes to `tx`.
    pub fn new(inner: W, tx: Sender<AcpEvent>) -> Self {
        Self { inner, tx }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for TeeWrite<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let me = self.get_mut();
        let n = match Pin::new(&mut me.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => n,
            other => return other,
        };
        emit(&me.tx, WireDir::ToAgent, &buf[..n]);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Wraps the agent's stdout: tees every read as [`WireDir::FromAgent`].
pub struct TeeRead<R> {
    inner: R,
    tx: Sender<AcpEvent>,
}

impl<R> TeeRead<R> {
    /// Wrap `inner`, copying reads to `tx`.
    pub fn new(inner: R, tx: Sender<AcpEvent>) -> Self {
        Self { inner, tx }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for TeeRead<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut me.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                emit(&me.tx, WireDir::FromAgent, &buf.filled()[before..]);
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
