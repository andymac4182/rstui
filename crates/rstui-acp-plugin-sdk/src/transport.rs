//! Transports: how a [`Message`] is framed.
//!
//! The protocol is transport-agnostic. [`StdioTransport`] is the default
//! (newline-delimited JSON-RPC over the process's stdin/stdout — exactly the
//! ACP/MCP stdio convention); [`IoTransport`] is the same framing over any
//! `BufRead`/`Write` pair, which the websocket transport and tests reuse.
//!
//! Both carry the JSON-RPC [`Message`] unchanged.

use std::io::{self, BufRead, BufReader, Read, Stdin, Stdout, Write};

use crate::jsonrpc::Message;

/// Frame ceiling for length-prefixed transports (16 MiB) — a malformed or
/// hostile length can never make us allocate unbounded.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

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

/// Length-prefixed JSON-RPC over any `Read` + `Write`: each message is a
/// big-endian `u32` byte count followed by the raw JSON bytes (no newline
/// scan, exact reads, JSON parsed straight from the byte slice). Same
/// JSON-RPC 2.0 semantics — only the framing is binary.
pub struct LpTransport<R: Read, W: Write> {
    reader: BufReader<R>,
    writer: W,
    /// Reused across `recv` so a steady message stream allocates once, not
    /// per frame (the hot-path win the profiler flagged).
    rbuf: Vec<u8>,
    /// Likewise reused across `send` (serialize in place, no temp `Vec`).
    wbuf: Vec<u8>,
}

impl<R: Read, W: Write> LpTransport<R, W> {
    /// Wraps a reader/writer pair (same shape as [`IoTransport`]).
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            rbuf: Vec::new(),
            wbuf: Vec::new(),
        }
    }
}

impl<R: Read, W: Write> Transport for LpTransport<R, W> {
    fn recv(&mut self) -> io::Result<Option<Message>> {
        let mut len = [0u8; 4];
        // Distinguish a clean EOF (no bytes at a frame boundary) from a
        // truncated frame (some bytes then EOF).
        let mut got = 0;
        while got < 4 {
            let n = self.reader.read(&mut len[got..])?;
            if n == 0 {
                return if got == 0 {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated length prefix",
                    ))
                };
            }
            got += n;
        }
        let n = u32::from_be_bytes(len);
        if n > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds 16 MiB cap",
            ));
        }
        let n = n as usize;
        self.rbuf.clear();
        self.rbuf.resize(n, 0);
        self.reader.read_exact(&mut self.rbuf)?;
        serde_json::from_slice(&self.rbuf)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn send(&mut self, msg: &Message) -> io::Result<()> {
        self.wbuf.clear();
        serde_json::to_writer(&mut self.wbuf, msg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let n = u32::try_from(self.wbuf.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
        self.writer.write_all(&n.to_be_bytes())?;
        self.writer.write_all(&self.wbuf)?;
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
