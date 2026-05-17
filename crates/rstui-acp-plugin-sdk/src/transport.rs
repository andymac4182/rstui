//! Transports: how a [`Message`] is framed.
//!
//! The protocol is transport-agnostic. [`StdioTransport`] is the default
//! (newline-delimited JSON-RPC over the process's stdin/stdout — exactly the
//! ACP/MCP stdio convention); [`IoTransport`] is the same framing over any
//! `BufRead`/`Write` pair, which the websocket transport and tests reuse.
//!
//! Both carry the JSON-RPC [`Message`] unchanged.

use std::io::{self, BufRead, BufReader, Stdin, Stdout, Write};

use crate::jsonrpc::Message;

/// A bidirectional JSON-RPC message channel.
pub trait Transport {
    /// Reads the next message, or `Ok(None)` at end-of-stream.
    ///
    /// # Errors
    ///
    /// I/O or malformed-line failures.
    fn recv(&mut self) -> io::Result<Option<Message>>;

    /// Writes one message and flushes it.
    ///
    /// # Errors
    ///
    /// I/O failures.
    fn send(&mut self, msg: &Message) -> io::Result<()>;
}

/// Newline-delimited JSON-RPC over any `BufRead` + `Write`.
pub struct IoTransport<R: BufRead, W: Write> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> IoTransport<R, W> {
    /// Wraps an existing reader/writer pair.
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
}

impl<R: BufRead, W: Write> Transport for IoTransport<R, W> {
    fn recv(&mut self) -> io::Result<Option<Message>> {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                return Ok(None); // EOF
            }
            if line.trim().is_empty() {
                continue;
            }
            return Message::decode_line(&line)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
        }
    }

    fn send(&mut self, msg: &Message) -> io::Result<()> {
        self.writer.write_all(msg.encode_line().as_bytes())?;
        self.writer.flush()
    }
}

/// The default plugin transport: this process's locked stdin/stdout.
pub struct StdioTransport {
    inner: IoTransport<BufReader<Stdin>, Stdout>,
}

impl StdioTransport {
    /// Binds to `std::io::stdin()` / `std::io::stdout()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: IoTransport::new(BufReader::new(io::stdin()), io::stdout()),
        }
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for StdioTransport {
    fn recv(&mut self) -> io::Result<Option<Message>> {
        self.inner.recv()
    }
    fn send(&mut self, msg: &Message) -> io::Result<()> {
        self.inner.send(msg)
    }
}
