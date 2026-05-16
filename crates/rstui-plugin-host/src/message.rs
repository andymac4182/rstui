//! The capability-call payload codec: hand-rolled, fail-closed
//! encode/decode of [`CapabilityRequest`] and [`CapabilityResponse`] into the
//! opaque `Vec<u8>` payload that [`crate::protocol`] carries in each frame
//! (ADR 0007 §2/§4).
//!
//! # Layering
//!
//! [`crate::protocol`] is the *frame* codec: it reads/writes the fixed
//! `[4B length][1B type][16B correlation-id][payload]` envelope and treats the
//! payload as opaque bytes.  **This module is the *payload* codec**: it gives
//! those bytes meaning for the two capability-call message types
//! ([`CapabilityCall`](crate::protocol::MessageType::CapabilityCall) carries
//! an encoded [`CapabilityRequest`];
//! [`CapabilityResponse`](crate::protocol::MessageType::CapabilityResponse)
//! carries an encoded [`CapabilityResponse`]).  The two layers are deliberately
//! kept separate: `protocol` never imports this module, and this module has no
//! dependency on `protocol`.
//!
//! # Wire format
//!
//! All integers are **big-endian**.  The format is self-describing: the first
//! byte of every payload is a **variant tag** that selects the remaining layout.
//! Two independent tag spaces are used:
//!
//! | Tag  | Meaning                              |
//! |------|--------------------------------------|
//! | `0x10` | `CapabilityRequest::Filesystem`    |
//! | `0x11` | `CapabilityRequest::Network`       |
//! | `0x12` | `CapabilityRequest::Command`       |
//! | `0x13` | `CapabilityRequest::Env`           |
//! | `0x20` | `CapabilityResponse::Ok`           |
//! | `0x21` | `CapabilityResponse::Denied`       |
//! | `0x22` | `CapabilityResponse::Failed`       |
//!
//! Primitive encodings used throughout:
//!
//! - **`u8` (tag, `FsMode`):** 1 byte.
//!   - `FsMode::Read` → `0`; `FsMode::Write` → `1`; any other byte →
//!     [`MessageError::UnknownTag`].
//! - **`u16` (port):** 2 bytes, big-endian.
//! - **`String`:** `u32` byte-length (big-endian), then exactly that many
//!   UTF-8 bytes.  On decode: the length is checked against the remaining
//!   slice *before* any allocation ([`MessageError::Truncated`] if
//!   `len > remaining`); then the bytes are validated as UTF-8
//!   ([`MessageError::BadUtf8`] on failure).
//! - **`Vec<String>` (args):** `u32` count, then each string as above.
//! - **`Vec<u8>` (payload):** `u32` byte-length, then exactly that many bytes.
//!
//! Full per-variant layouts (after the leading tag byte):
//!
//! ```text
//! Filesystem: [tag 0x10][FsMode u8][path_len u32BE][path UTF-8]
//!                       [contents_len u32BE][contents bytes]
//! Network:    [tag 0x11][host_len u32BE][host UTF-8][port u16BE]
//! Command:    [tag 0x12][program_len u32BE][program UTF-8]
//!                       [argc u32BE]([arg_len u32BE][arg UTF-8])*
//! Env:        [tag 0x13][key_len u32BE][key UTF-8]
//!
//! Ok:         [tag 0x20][payload_len u32BE][payload bytes]
//! Denied:     [tag 0x21][reason_len u32BE][reason UTF-8]
//! Failed:     [tag 0x22][error_len  u32BE][error UTF-8]
//! ```
//!
//! # Fail-closed rules (ADR 0007 §4)
//!
//! - Unknown tag byte → [`MessageError::UnknownTag`].
//! - Input shorter than the format requires → [`MessageError::Truncated`].
//! - Leftover bytes after a complete decode → [`MessageError::TrailingBytes`]
//!   (trailing data is never ignored; it is evidence of a format violation).
//! - Invalid UTF-8 in a string field → [`MessageError::BadUtf8`].
//! - A `u32` length field whose value exceeds the remaining slice length →
//!   [`MessageError::Truncated`] — the check happens *before* any allocation
//!   so an attacker-controlled large-length field cannot force a huge heap
//!   allocation.
//!
//! # Example
//!
//! ```
//! use rstui_plugin_host::capability::{CapabilityRequest, FsMode};
//! use rstui_plugin_host::message::{decode_request, encode_request};
//!
//! let req = CapabilityRequest::Filesystem {
//!     mode: FsMode::Read,
//!     path: "/srv/data/report.csv".into(),
//!     contents: Vec::new(),
//! };
//!
//! let bytes = encode_request(&req);
//! let decoded = decode_request(&bytes).expect("round-trip must succeed");
//! assert_eq!(decoded, req);
//! ```

use std::fmt;
use std::path::PathBuf;

use crate::capability::{CapabilityRequest, FsMode};

// ──────────────────────────────────────────────────────────────────────────────
// Variant tags
// ──────────────────────────────────────────────────────────────────────────────

/// Tag byte for [`CapabilityRequest::Filesystem`].
const TAG_REQ_FILESYSTEM: u8 = 0x10;
/// Tag byte for [`CapabilityRequest::Network`].
const TAG_REQ_NETWORK: u8 = 0x11;
/// Tag byte for [`CapabilityRequest::Command`].
const TAG_REQ_COMMAND: u8 = 0x12;
/// Tag byte for [`CapabilityRequest::Env`].
const TAG_REQ_ENV: u8 = 0x13;

/// Tag byte for [`CapabilityResponse::Ok`].
const TAG_RESP_OK: u8 = 0x20;
/// Tag byte for [`CapabilityResponse::Denied`].
const TAG_RESP_DENIED: u8 = 0x21;
/// Tag byte for [`CapabilityResponse::Failed`].
const TAG_RESP_FAILED: u8 = 0x22;

/// Wire byte for [`FsMode::Read`].
const FSMODE_READ: u8 = 0;
/// Wire byte for [`FsMode::Write`].
const FSMODE_WRITE: u8 = 1;

// ──────────────────────────────────────────────────────────────────────────────
// CapabilityResponse
// ──────────────────────────────────────────────────────────────────────────────

/// The host's reply to a plugin's capability call.
///
/// This is the *result* side of the request/response pair:
/// [`CapabilityRequest`] encodes what the plugin wants to do;
/// `CapabilityResponse` encodes what the host decided and, if permitted, what
/// happened.
///
/// The three outcomes map to distinct tag bytes (`0x20`/`0x21`/`0x22`) in the
/// wire encoding so the plugin can parse the tag before committing to the full
/// decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityResponse {
    /// The permission policy permitted the request **and** the host effect
    /// succeeded.  `payload` carries the effect result bytes (for example, the
    /// bytes read from a file, or the stdout of a command).  It may be empty
    /// when no result data is meaningful (e.g. a write that succeeded).
    Ok {
        /// Effect result bytes; may be empty.
        payload: Vec<u8>,
    },
    /// The permission policy **refused** the request before the host effect
    /// ran.  `reason` is a human-readable explanation (which policy rule
    /// triggered, what grant was absent, etc.).
    Denied {
        /// Why the request was denied.
        reason: String,
    },
    /// The permission policy permitted the request but the host effect
    /// **failed** (for example, a file was not found, or a command exited
    /// non-zero).  `error` describes the failure.
    Failed {
        /// The error the host effect produced.
        error: String,
    },
}

// ──────────────────────────────────────────────────────────────────────────────
// MessageError
// ──────────────────────────────────────────────────────────────────────────────

/// A decode error produced by [`decode_request`] or [`decode_response`].
///
/// Every variant signals a fatal condition (ADR 0007 §4 fail-closed rule):
/// the plugin connection is terminated on any error; there is no partial
/// parse, skip-and-continue, or best-effort decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageError {
    /// The leading tag byte (or an `FsMode` byte) is not a value assigned to
    /// any variant.  `u8` is the byte that was found on the wire.
    UnknownTag(u8),
    /// The input ended before the format requires.  This includes both literal
    /// EOF mid-field and a length field whose value exceeds the remaining slice
    /// (checked *before* any allocation to prevent attacker-controlled huge
    /// allocations).
    Truncated,
    /// Bytes remain after a complete, valid decode.  Trailing data is rejected
    /// rather than silently ignored to prevent format drift and encoding errors
    /// from going undetected.
    TrailingBytes,
    /// A string field's bytes are not valid UTF-8.
    BadUtf8,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTag(b) => write!(f, "unknown tag byte: 0x{b:02X}"),
            Self::Truncated => f.write_str("input truncated: ended before the field was complete"),
            Self::TrailingBytes => {
                f.write_str("trailing bytes after a complete decode: encoding is malformed")
            }
            Self::BadUtf8 => f.write_str("string field contains invalid UTF-8"),
        }
    }
}

impl std::error::Error for MessageError {}

// ──────────────────────────────────────────────────────────────────────────────
// Encode: CapabilityRequest
// ──────────────────────────────────────────────────────────────────────────────

/// Encodes `req` to a self-describing binary payload.
///
/// The returned `Vec<u8>` is suitable for use as the `payload` field of a
/// [`Frame`](crate::protocol::Frame) with message type
/// [`CapabilityCall`](crate::protocol::MessageType::CapabilityCall).
///
/// Encoding never fails — every `CapabilityRequest` has a well-defined wire
/// representation.
#[must_use]
pub fn encode_request(req: &CapabilityRequest) -> Vec<u8> {
    let mut out = Vec::new();
    match req {
        CapabilityRequest::Filesystem {
            mode,
            path,
            contents,
        } => {
            out.push(TAG_REQ_FILESYSTEM);
            push_fsmode(&mut out, *mode);
            push_str(&mut out, path.to_string_lossy().as_ref());
            push_bytes(&mut out, contents);
        }
        CapabilityRequest::Network { host, port } => {
            out.push(TAG_REQ_NETWORK);
            push_str(&mut out, host);
            out.extend_from_slice(&port.to_be_bytes());
        }
        CapabilityRequest::Command { program, args } => {
            out.push(TAG_REQ_COMMAND);
            push_str(&mut out, program);
            push_string_vec(&mut out, args);
        }
        CapabilityRequest::Env { key } => {
            out.push(TAG_REQ_ENV);
            push_str(&mut out, key);
        }
    }
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Decode: CapabilityRequest
// ──────────────────────────────────────────────────────────────────────────────

/// Decodes a [`CapabilityRequest`] from `bytes`.
///
/// `bytes` must be exactly the payload produced by [`encode_request`]: any
/// unrecognised tag, truncation, trailing bytes, or invalid UTF-8 produces an
/// appropriate [`MessageError`] variant.  This function **never** attempts to
/// allocate a buffer before verifying the claimed length fits within the
/// remaining input.
///
/// # Errors
///
/// - [`MessageError::UnknownTag`] — the first byte is not a known request tag.
/// - [`MessageError::Truncated`] — the input is shorter than the format
///   requires, or a length field claims more bytes than remain.
/// - [`MessageError::TrailingBytes`] — bytes remain after a complete decode.
/// - [`MessageError::BadUtf8`] — a string field is not valid UTF-8.
pub fn decode_request(bytes: &[u8]) -> Result<CapabilityRequest, MessageError> {
    let mut r = Reader::new(bytes);
    let tag = r.read_u8()?;
    let req = match tag {
        TAG_REQ_FILESYSTEM => {
            let mode = read_fsmode(&mut r)?;
            let path = PathBuf::from(read_string(&mut r)?);
            let contents = r.read_bytes()?;
            CapabilityRequest::Filesystem {
                mode,
                path,
                contents,
            }
        }
        TAG_REQ_NETWORK => {
            let host = read_string(&mut r)?;
            let port = r.read_u16()?;
            CapabilityRequest::Network { host, port }
        }
        TAG_REQ_COMMAND => {
            let program = read_string(&mut r)?;
            let args = read_string_vec(&mut r)?;
            CapabilityRequest::Command { program, args }
        }
        TAG_REQ_ENV => {
            let key = read_string(&mut r)?;
            CapabilityRequest::Env { key }
        }
        other => return Err(MessageError::UnknownTag(other)),
    };
    r.expect_exhausted()?;
    Ok(req)
}

// ──────────────────────────────────────────────────────────────────────────────
// Encode: CapabilityResponse
// ──────────────────────────────────────────────────────────────────────────────

/// Encodes `resp` to a self-describing binary payload.
///
/// The returned `Vec<u8>` is suitable for use as the `payload` field of a
/// [`Frame`](crate::protocol::Frame) with message type
/// [`CapabilityResponse`](crate::protocol::MessageType::CapabilityResponse).
///
/// Encoding never fails.
#[must_use]
pub fn encode_response(resp: &CapabilityResponse) -> Vec<u8> {
    let mut out = Vec::new();
    match resp {
        CapabilityResponse::Ok { payload } => {
            out.push(TAG_RESP_OK);
            push_bytes(&mut out, payload);
        }
        CapabilityResponse::Denied { reason } => {
            out.push(TAG_RESP_DENIED);
            push_str(&mut out, reason);
        }
        CapabilityResponse::Failed { error } => {
            out.push(TAG_RESP_FAILED);
            push_str(&mut out, error);
        }
    }
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Decode: CapabilityResponse
// ──────────────────────────────────────────────────────────────────────────────

/// Decodes a [`CapabilityResponse`] from `bytes`.
///
/// `bytes` must be exactly the payload produced by [`encode_response`]: any
/// unrecognised tag, truncation, trailing bytes, or invalid UTF-8 produces an
/// appropriate [`MessageError`] variant.
///
/// # Errors
///
/// - [`MessageError::UnknownTag`] — the first byte is not a known response tag.
/// - [`MessageError::Truncated`] — the input is shorter than the format
///   requires, or a length field claims more bytes than remain.
/// - [`MessageError::TrailingBytes`] — bytes remain after a complete decode.
/// - [`MessageError::BadUtf8`] — a string field is not valid UTF-8.
pub fn decode_response(bytes: &[u8]) -> Result<CapabilityResponse, MessageError> {
    let mut r = Reader::new(bytes);
    let tag = r.read_u8()?;
    let resp = match tag {
        TAG_RESP_OK => {
            let payload = r.read_bytes()?;
            CapabilityResponse::Ok { payload }
        }
        TAG_RESP_DENIED => {
            let reason = read_string(&mut r)?;
            CapabilityResponse::Denied { reason }
        }
        TAG_RESP_FAILED => {
            let error = read_string(&mut r)?;
            CapabilityResponse::Failed { error }
        }
        other => return Err(MessageError::UnknownTag(other)),
    };
    r.expect_exhausted()?;
    Ok(resp)
}

// ──────────────────────────────────────────────────────────────────────────────
// Encode primitives
// ──────────────────────────────────────────────────────────────────────────────

/// Appends a string: `u32` byte-length then the UTF-8 bytes.
fn push_str(out: &mut Vec<u8>, s: &str) {
    let len = s.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Appends a byte slice: `u32` byte-length then the bytes.
fn push_bytes(out: &mut Vec<u8>, b: &[u8]) {
    let len = b.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(b);
}

/// Appends `mode` as a single wire byte.
fn push_fsmode(out: &mut Vec<u8>, mode: FsMode) {
    out.push(match mode {
        FsMode::Read => FSMODE_READ,
        FsMode::Write => FSMODE_WRITE,
    });
}

/// Appends a `Vec<String>`: `u32` count then each string via [`push_str`].
fn push_string_vec(out: &mut Vec<u8>, v: &[String]) {
    let count = v.len() as u32;
    out.extend_from_slice(&count.to_be_bytes());
    for s in v {
        push_str(out, s);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Decode primitives (via Reader)
// ──────────────────────────────────────────────────────────────────────────────

/// A cursor over a borrowed byte slice.
///
/// All reads advance the internal position.  Every method checks that
/// sufficient bytes remain *before* any allocation or copy so an
/// attacker-controlled length field cannot force a huge allocation.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Remaining unread bytes.
    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Reads exactly `n` bytes, advancing the position.
    fn read_raw(&mut self, n: usize) -> Result<&'a [u8], MessageError> {
        if self.remaining() < n {
            return Err(MessageError::Truncated);
        }
        let slice = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Reads a single byte.
    fn read_u8(&mut self) -> Result<u8, MessageError> {
        let bytes = self.read_raw(1)?;
        Ok(bytes[0])
    }

    /// Reads a big-endian `u16`.
    fn read_u16(&mut self) -> Result<u16, MessageError> {
        let bytes = self.read_raw(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a big-endian `u32` length prefix followed by that many bytes,
    /// returning the bytes.  **Checks the claimed length against the remaining
    /// slice before allocating.**
    fn read_bytes(&mut self) -> Result<Vec<u8>, MessageError> {
        let len_bytes = self.read_raw(4)?;
        let len =
            u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        // Bounds check before allocation — prevents attacker-controlled OOM.
        if len > self.remaining() {
            return Err(MessageError::Truncated);
        }
        Ok(self.read_raw(len)?.to_vec())
    }

    /// Returns `Ok(())` if all bytes have been consumed, or
    /// [`MessageError::TrailingBytes`] if any remain.
    fn expect_exhausted(&self) -> Result<(), MessageError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(MessageError::TrailingBytes)
        }
    }
}

/// Reads a length-prefixed UTF-8 string from `r`.
fn read_string(r: &mut Reader<'_>) -> Result<String, MessageError> {
    let bytes = r.read_bytes()?;
    String::from_utf8(bytes).map_err(|_| MessageError::BadUtf8)
}

/// Reads a length-prefixed [`FsMode`] byte from `r`.
fn read_fsmode(r: &mut Reader<'_>) -> Result<FsMode, MessageError> {
    match r.read_u8()? {
        FSMODE_READ => Ok(FsMode::Read),
        FSMODE_WRITE => Ok(FsMode::Write),
        other => Err(MessageError::UnknownTag(other)),
    }
}

/// Reads a `u32`-count sequence of length-prefixed strings from `r`.
fn read_string_vec(r: &mut Reader<'_>) -> Result<Vec<String>, MessageError> {
    let count_bytes = r.read_raw(4)?;
    let count = u32::from_be_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
    ]) as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(read_string(r)?);
    }
    Ok(out)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── round-trip helpers ────────────────────────────────────────────────────

    fn req_round_trip(req: &CapabilityRequest) -> CapabilityRequest {
        let encoded = encode_request(req);
        decode_request(&encoded).expect("round-trip should succeed")
    }

    fn resp_round_trip(resp: &CapabilityResponse) -> CapabilityResponse {
        let encoded = encode_response(resp);
        decode_response(&encoded).expect("round-trip should succeed")
    }

    // ── CapabilityRequest round-trips ─────────────────────────────────────────

    #[test]
    fn filesystem_read_round_trips() {
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: "/srv/data/report.csv".into(),
            contents: Vec::new(),
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn filesystem_write_round_trips() {
        // A write carries a payload; the codec must round-trip the bytes
        // (including a NUL and high bytes) verbatim alongside path+mode.
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Write,
            path: "/tmp/output".into(),
            contents: b"line1\n\0\xde\xad\xbe\xef".to_vec(),
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn filesystem_unicode_path_round_trips() {
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: "/データ/ファイル.txt".into(),
            contents: Vec::new(),
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn network_round_trips_basic() {
        let req = CapabilityRequest::Network {
            host: "example.com".into(),
            port: 443,
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn network_round_trips_port_min() {
        let req = CapabilityRequest::Network {
            host: "localhost".into(),
            port: 0,
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn network_round_trips_port_max() {
        let req = CapabilityRequest::Network {
            host: "10.0.0.1".into(),
            port: 65535,
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn command_empty_args_round_trips() {
        let req = CapabilityRequest::Command {
            program: "git".into(),
            args: vec![],
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn command_many_args_round_trips() {
        let req = CapabilityRequest::Command {
            program: "/usr/bin/env".into(),
            args: vec!["a".into(), "bb".into(), "ccc".into()],
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn command_args_with_spaces_and_unicode_round_trips() {
        let req = CapabilityRequest::Command {
            program: "echo".into(),
            args: vec![
                "hello world".into(),
                "日本語".into(),
                "arg with spaces".into(),
            ],
        };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn env_round_trips() {
        let req = CapabilityRequest::Env { key: "HOME".into() };
        assert_eq!(req_round_trip(&req), req);
    }

    #[test]
    fn env_unicode_key_round_trips() {
        let req = CapabilityRequest::Env {
            key: "КЛЮЧ_ЗНАЧЕНИЕ".into(),
        };
        assert_eq!(req_round_trip(&req), req);
    }

    // ── CapabilityResponse round-trips ────────────────────────────────────────

    #[test]
    fn response_ok_empty_payload_round_trips() {
        let resp = CapabilityResponse::Ok { payload: vec![] };
        assert_eq!(resp_round_trip(&resp), resp);
    }

    #[test]
    fn response_ok_large_payload_round_trips() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let resp = CapabilityResponse::Ok {
            payload: payload.clone(),
        };
        assert_eq!(resp_round_trip(&resp), resp);
    }

    #[test]
    fn response_denied_round_trips() {
        let resp = CapabilityResponse::Denied {
            reason: "path is outside the granted root".into(),
        };
        assert_eq!(resp_round_trip(&resp), resp);
    }

    #[test]
    fn response_denied_unicode_reason_round_trips() {
        let resp = CapabilityResponse::Denied {
            reason: "アクセス拒否 — 許可されていないパス".into(),
        };
        assert_eq!(resp_round_trip(&resp), resp);
    }

    #[test]
    fn response_failed_round_trips() {
        let resp = CapabilityResponse::Failed {
            error: "No such file or directory (os error 2)".into(),
        };
        assert_eq!(resp_round_trip(&resp), resp);
    }

    // ── adversarial decode: CapabilityRequest ─────────────────────────────────

    #[test]
    fn empty_input_is_truncated() {
        assert_eq!(decode_request(&[]), Err(MessageError::Truncated));
        assert_eq!(decode_response(&[]), Err(MessageError::Truncated));
    }

    #[test]
    fn unknown_tag_byte_is_rejected_for_request() {
        assert_eq!(decode_request(&[0x00]), Err(MessageError::UnknownTag(0x00)));
        assert_eq!(decode_request(&[0xFF]), Err(MessageError::UnknownTag(0xFF)));
        // Response tags in the request stream are unknown to the request decoder.
        assert_eq!(
            decode_request(&[TAG_RESP_OK]),
            Err(MessageError::UnknownTag(TAG_RESP_OK))
        );
    }

    #[test]
    fn unknown_tag_byte_is_rejected_for_response() {
        assert_eq!(
            decode_response(&[0x00]),
            Err(MessageError::UnknownTag(0x00))
        );
        assert_eq!(
            decode_response(&[0xFF]),
            Err(MessageError::UnknownTag(0xFF))
        );
        // Request tags in the response stream are unknown to the response decoder.
        assert_eq!(
            decode_response(&[TAG_REQ_FILESYSTEM]),
            Err(MessageError::UnknownTag(TAG_REQ_FILESYSTEM))
        );
    }

    #[test]
    fn truncated_mid_string_length_field_is_truncated() {
        // Env tag then only 3 of the 4 length bytes.
        let buf = [TAG_REQ_ENV, 0x00, 0x00, 0x00];
        assert_eq!(decode_request(&buf), Err(MessageError::Truncated));
    }

    #[test]
    fn truncated_mid_string_body_is_truncated() {
        // Env tag, length = 10 (big-endian), only 5 bytes of body.
        let mut buf = vec![TAG_REQ_ENV];
        buf.extend_from_slice(&10u32.to_be_bytes());
        buf.extend_from_slice(b"hello"); // only 5 of the 10 claimed bytes
        assert_eq!(decode_request(&buf), Err(MessageError::Truncated));
    }

    #[test]
    fn truncated_mid_u16_port_is_truncated() {
        // Network tag, valid host "h", then only 1 byte of the 2-byte port.
        let mut buf = vec![TAG_REQ_NETWORK];
        push_str(&mut buf, "h");
        buf.push(0x01); // only one byte of the u16 port
        assert_eq!(decode_request(&buf), Err(MessageError::Truncated));
    }

    #[test]
    fn truncated_mid_vec_count_is_truncated() {
        // Command tag, valid program, then only 3 of the 4 count bytes.
        let mut buf = vec![TAG_REQ_COMMAND];
        push_str(&mut buf, "git");
        buf.extend_from_slice(&[0x00, 0x00, 0x00]); // only 3 count bytes
        assert_eq!(decode_request(&buf), Err(MessageError::Truncated));
    }

    #[test]
    fn trailing_extra_byte_after_valid_encoding_is_rejected() {
        let req = CapabilityRequest::Env { key: "PATH".into() };
        let mut encoded = encode_request(&req);
        encoded.push(0xFF); // inject a trailing byte
        assert_eq!(decode_request(&encoded), Err(MessageError::TrailingBytes));

        let resp = CapabilityResponse::Denied {
            reason: "no".into(),
        };
        let mut encoded = encode_response(&resp);
        encoded.push(0x42);
        assert_eq!(decode_response(&encoded), Err(MessageError::TrailingBytes));
    }

    #[test]
    fn invalid_utf8_in_string_field_is_bad_utf8() {
        // Env tag, length = 3, then 3 non-UTF-8 bytes (overlong / continuation
        // bytes without a start byte).
        let mut buf = vec![TAG_REQ_ENV];
        buf.extend_from_slice(&3u32.to_be_bytes());
        buf.extend_from_slice(&[0xFF, 0xFE, 0xFD]);
        assert_eq!(decode_request(&buf), Err(MessageError::BadUtf8));
    }

    #[test]
    fn invalid_utf8_in_response_string_is_bad_utf8() {
        let mut buf = vec![TAG_RESP_DENIED];
        buf.extend_from_slice(&2u32.to_be_bytes());
        buf.extend_from_slice(&[0xC3, 0x28]); // invalid 2-byte sequence
        assert_eq!(decode_response(&buf), Err(MessageError::BadUtf8));
    }

    #[test]
    fn huge_length_with_no_body_is_truncated_without_allocation() {
        // Env tag, then a 4-byte length claiming u32::MAX bytes (far more than
        // the remaining 0 body bytes).  The decoder must return Truncated
        // without attempting to allocate u32::MAX bytes.
        let mut buf = vec![TAG_REQ_ENV];
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        // No body bytes at all — remaining after length field: 0.
        assert_eq!(decode_request(&buf), Err(MessageError::Truncated));

        // Same check for response (Ok payload).
        let mut buf2 = vec![TAG_RESP_OK];
        buf2.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(decode_response(&buf2), Err(MessageError::Truncated));
    }

    #[test]
    fn unknown_fsmode_byte_is_unknown_tag() {
        // Filesystem tag, then an invalid mode byte (2 is unassigned).
        let buf = [TAG_REQ_FILESYSTEM, 0x02];
        assert_eq!(decode_request(&buf), Err(MessageError::UnknownTag(0x02)));
    }

    // ── byte-level wire-layout pin test ──────────────────────────────────────

    /// Pins the exact wire bytes of a known [`CapabilityRequest::Env`] so any
    /// accidental format change produces a compile-time-visible test failure.
    ///
    /// Layout: `[0x13][0x00 0x00 0x00 0x04][H O M E]`
    ///          ^^^^   ^^^^^^^^^^^^^^^^^^^   ^^^^^^^
    ///          tag    key length = 4 BE      "HOME"
    #[test]
    fn wire_layout_env_request_is_exact() {
        let req = CapabilityRequest::Env { key: "HOME".into() };
        let bytes = encode_request(&req);
        assert_eq!(
            bytes,
            &[
                TAG_REQ_ENV, // 0x13
                0x00,
                0x00,
                0x00,
                0x04, // key length = 4, big-endian
                b'H',
                b'O',
                b'M',
                b'E', // "HOME"
            ],
            "wire layout of Env(\"HOME\") must match exactly"
        );
        // And the reverse direction also recovers the original.
        assert_eq!(decode_request(&bytes).unwrap(), req);
    }

    /// Pins the exact wire bytes of a [`CapabilityResponse::Ok`] with a
    /// known payload so the format is locked at the byte level.
    ///
    /// Layout: `[0x20][0x00 0x00 0x00 0x03][0xAA 0xBB 0xCC]`
    #[test]
    fn wire_layout_ok_response_is_exact() {
        let resp = CapabilityResponse::Ok {
            payload: vec![0xAA, 0xBB, 0xCC],
        };
        let bytes = encode_response(&resp);
        assert_eq!(
            bytes,
            &[
                TAG_RESP_OK, // 0x20
                0x00,
                0x00,
                0x00,
                0x03, // payload length = 3, big-endian
                0xAA,
                0xBB,
                0xCC, // the three payload bytes
            ],
            "wire layout of Ok([0xAA,0xBB,0xCC]) must match exactly"
        );
        assert_eq!(decode_response(&bytes).unwrap(), resp);
    }

    // ── MessageError display ──────────────────────────────────────────────────

    #[test]
    fn message_error_display_is_informative() {
        let s = MessageError::UnknownTag(0xAB).to_string();
        assert!(
            s.to_lowercase().contains("ab"),
            "display should include the byte in hex"
        );

        let s = MessageError::Truncated.to_string();
        assert!(!s.is_empty());

        let s = MessageError::TrailingBytes.to_string();
        assert!(!s.is_empty());

        let s = MessageError::BadUtf8.to_string();
        assert!(!s.is_empty());
    }

    #[test]
    fn message_error_implements_std_error() {
        // std::error::Error is implemented — just check it compiles and that
        // source() returns None (no chained cause).
        use std::error::Error;
        let err: &dyn Error = &MessageError::Truncated;
        assert!(err.source().is_none());
    }
}
