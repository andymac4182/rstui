//! The hand-rolled length-prefixed, *fail-closed* host↔plugin frame codec
//! (ADR 0007 §4).
//!
//! # Wire layout
//!
//! Every message is wrapped in a fixed-envelope frame:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  0  1  2  3   │  4   │  5  6 … 20   │  21 …                   │
//! │  length u32BE │ type │ corr-id [16B] │ payload bytes           │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - **Length (4 bytes, big-endian u32):** byte count of everything that
//!   follows — `1 + 16 + payload_len`.  The four length bytes themselves are
//!   *excluded* from this count, matching secure-exec's `ipc_binary.rs`.
//! - **Message type (1 byte):** distinguishes host→plugin (`0x01`–`0x04`)
//!   from plugin→host (`0x81`–`0x84`) at the wire level.  A frame carrying
//!   the wrong direction code is detectable before the payload is touched.
//! - **Correlation id (16 bytes):** opaque identifier that routes a response
//!   back to the request that originated it.  The host and plugin each choose
//!   ids for the frames they originate; the responder echoes the id unchanged.
//! - **Payload (remaining bytes):** opaque `Vec<u8>` at this layer.  The host
//!   assigns meaning elsewhere; `protocol` never interprets it.
//!
//! # Fail-closed rule
//!
//! Any framing or decode error **terminates the plugin connection** — there is
//! no skip-and-continue (ADR 0007 §4, mirroring secure-exec's explicit rule).
//! The caller is responsible for closing the pipe/process on every
//! [`ProtocolError`]; the error variants are designed to make each distinct
//! failure mode diagnosable.
//!
//! # Why the length is capped before allocation
//!
//! [`read_frame`] reads the 4-byte length header and immediately checks it
//! against [`MAX_FRAME_SIZE`] before allocating any buffer.  A plugin (or an
//! attacker controlling the byte stream) that writes a 4-byte header claiming
//! a 4 GiB body would otherwise force the host to attempt a multi-gigabyte
//! allocation, which is either a crash (OOM) or an effective denial-of-service.
//! By checking the length *first*, the host refuses the frame in O(1) with no
//! heap pressure, then closes the connection.
//!
//! # Stdout carries only frames
//!
//! The plugin's stdout pipe is a dedicated frame channel: every byte on it
//! must conform to this protocol.  Plugin diagnostic output (log lines, panic
//! messages) goes to **stderr**.  The host may stream stderr via a callback,
//! but that is the host's concern; this module neither reads stderr nor
//! interprets stdout as anything other than a sequence of frames.
//!
//! # Example
//!
//! ```
//! use std::io::Cursor;
//! use rstui_plugin_host::protocol::{Frame, MessageType, read_frame, write_frame};
//!
//! // Build a host→plugin Initialize frame with a known correlation id.
//! let correlation_id = [1u8; 16];
//! let payload = b"hello plugin".to_vec();
//! let frame = Frame::new(MessageType::Initialize, correlation_id, payload.clone());
//!
//! // Encode into a byte buffer, then decode back out.
//! let mut buf = Vec::new();
//! write_frame(&mut buf, &frame).expect("encode failed");
//!
//! let mut cursor = Cursor::new(buf);
//! let decoded = read_frame(&mut cursor).expect("decode failed");
//!
//! assert_eq!(decoded.message_type, frame.message_type);
//! assert_eq!(decoded.correlation_id, correlation_id);
//! assert_eq!(decoded.payload, payload);
//! ```

use std::fmt;
use std::io::{self, Read, Write};

// ──────────────────────────────────────────────────────────────────────────────
// Constants
// ──────────────────────────────────────────────────────────────────────────────

/// Maximum number of bytes that the 4-byte length field may name (i.e. the
/// maximum size of `type + correlation_id + payload` combined).
///
/// Set to 16 MiB.  Enforced on both encode ([`write_frame`]) and decode
/// ([`read_frame`]); a frame whose named length exceeds this is rejected
/// before any body bytes are read or allocated.
pub const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024; // 16 MiB

/// Minimum valid length value: 1 (type) + 16 (correlation id) = 17.
const MIN_FRAME_LEN: u32 = 17;

// ──────────────────────────────────────────────────────────────────────────────
// MessageType
// ──────────────────────────────────────────────────────────────────────────────

/// Identifies the purpose of a frame and the direction it travels.
///
/// Host→plugin codes occupy `0x01`–`0x7F`; plugin→host codes occupy
/// `0x80`–`0xFF`.  The distinct ranges mean a misdirected frame (e.g. a
/// plugin echoing an `Initialize` back with a host-range type byte) is
/// detectable at the codec layer, before any payload interpretation.
///
/// # Wire codes
///
/// | Code   | Variant              | Direction     |
/// |--------|----------------------|---------------|
/// | `0x01` | `Initialize`         | host → plugin |
/// | `0x02` | `HookDispatch`       | host → plugin |
/// | `0x03` | `CapabilityResponse` | host → plugin |
/// | `0x04` | `Shutdown`           | host → plugin |
/// | `0x81` | `Ready`              | plugin → host |
/// | `0x82` | `CapabilityCall`     | plugin → host |
/// | `0x83` | `HookResult`         | plugin → host |
/// | `0x84` | `Log`                | plugin → host |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    // ── host → plugin ──────────────────────────────────────────────────────
    /// Host sends configuration and granted capability list to start up a
    /// plugin process.
    Initialize = 0x01,
    /// Host dispatches a hook invocation for the plugin to handle.
    HookDispatch = 0x02,
    /// Host delivers the result of a capability call the plugin requested.
    CapabilityResponse = 0x03,
    /// Host requests an orderly shutdown; the plugin should flush and exit.
    Shutdown = 0x04,

    // ── plugin → host ──────────────────────────────────────────────────────
    /// Plugin signals that it finished initialisation and is ready to receive
    /// hooks.
    Ready = 0x81,
    /// Plugin requests the host to perform a capability-gated side effect.
    CapabilityCall = 0x82,
    /// Plugin delivers the result of a hook it was dispatched.
    HookResult = 0x83,
    /// Plugin sends a diagnostic log line (structured or plain text).
    Log = 0x84,
}

impl MessageType {
    /// Encodes this type to its wire byte.
    #[must_use]
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Decodes a wire byte into a [`MessageType`].
    ///
    /// Returns `Err(`[`ProtocolError::UnknownMessageType`]`)` for any byte not
    /// assigned to a variant.  Unknown codes are rejected rather than
    /// defaulted — fail-closed (ADR 0007 §4).
    pub fn from_byte(b: u8) -> Result<Self, ProtocolError> {
        match b {
            0x01 => Ok(Self::Initialize),
            0x02 => Ok(Self::HookDispatch),
            0x03 => Ok(Self::CapabilityResponse),
            0x04 => Ok(Self::Shutdown),
            0x81 => Ok(Self::Ready),
            0x82 => Ok(Self::CapabilityCall),
            0x83 => Ok(Self::HookResult),
            0x84 => Ok(Self::Log),
            other => Err(ProtocolError::UnknownMessageType(other)),
        }
    }

    /// Returns `true` if this message type originates from the **host** and
    /// travels to the plugin.
    ///
    /// Callers can assert direction immediately after [`read_frame`] to
    /// detect misdirected frames and close the connection before touching the
    /// payload.
    #[must_use]
    pub fn from_host(self) -> bool {
        matches!(
            self,
            Self::Initialize | Self::HookDispatch | Self::CapabilityResponse | Self::Shutdown
        )
    }

    /// Returns `true` if this message type originates from the **plugin** and
    /// travels to the host.
    ///
    /// Symmetric with [`from_host`](Self::from_host); both helpers cover the
    /// full variant set so `from_host() || from_plugin()` is always `true`.
    #[must_use]
    pub fn from_plugin(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::CapabilityCall | Self::HookResult | Self::Log
        )
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Initialize => "Initialize",
            Self::HookDispatch => "HookDispatch",
            Self::CapabilityResponse => "CapabilityResponse",
            Self::Shutdown => "Shutdown",
            Self::Ready => "Ready",
            Self::CapabilityCall => "CapabilityCall",
            Self::HookResult => "HookResult",
            Self::Log => "Log",
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Frame
// ──────────────────────────────────────────────────────────────────────────────

/// A single host↔plugin protocol message with its wire-decoded fields.
///
/// `Frame` is the unit the codec produces and consumes.  The `payload` field
/// is opaque `Vec<u8>` at this layer; the host assigns meaning to its bytes
/// (for each [`MessageType`]) in a higher-level module that does not belong
/// here.
///
/// This module never references [`crate::capability`] or [`crate::manifest`]:
/// the frame layer is intentionally decoupled so it can be tested and reasoned
/// about independently of the capability model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The message type, which identifies direction and purpose.
    pub message_type: MessageType,
    /// 16-byte opaque identifier.  The sender chooses it; the receiver echoes
    /// it in any response so the originator can correlate replies to requests.
    pub correlation_id: [u8; 16],
    /// Opaque body.  Length is bounded by [`MAX_FRAME_SIZE`] minus 17 (the
    /// one type byte and the 16 id bytes).
    pub payload: Vec<u8>,
}

impl Frame {
    /// Constructs a new `Frame` from its constituent parts.
    ///
    /// This is a plain field-setting constructor; it does **not** validate
    /// `payload` length.  [`write_frame`] enforces [`MAX_FRAME_SIZE`] at
    /// encode time.
    #[must_use]
    pub fn new(message_type: MessageType, correlation_id: [u8; 16], payload: Vec<u8>) -> Self {
        Self {
            message_type,
            correlation_id,
            payload,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Codec
// ──────────────────────────────────────────────────────────────────────────────

/// Encodes `frame` and writes it atomically to `w`.
///
/// The full frame bytes are assembled in memory first, the length is enforced
/// against [`MAX_FRAME_SIZE`], and then a single [`Write::write_all`] commits
/// the entire frame (or nothing at all if the pre-flight check fails).  This
/// matches secure-exec's "length prefix written last" property: either the
/// complete, valid frame appears on the pipe or nothing does.  After a
/// successful write the writer is flushed so the peer receives the frame
/// immediately.
///
/// # Errors
///
/// - [`ProtocolError::FrameTooLarge`] — `1 + 16 + payload.len()` exceeds
///   [`MAX_FRAME_SIZE`].  Nothing is written to `w`.
/// - [`ProtocolError::Io`] — the underlying write or flush failed.
///
/// # Example
///
/// ```
/// use std::io::Cursor;
/// use rstui_plugin_host::protocol::{Frame, MessageType, read_frame, write_frame};
///
/// let frame = Frame::new(MessageType::Shutdown, [0u8; 16], vec![]);
/// let mut buf = Vec::new();
/// write_frame(&mut buf, &frame).unwrap();
/// assert!(!buf.is_empty());
///
/// let decoded = read_frame(&mut Cursor::new(buf)).unwrap();
/// assert_eq!(decoded.message_type, MessageType::Shutdown);
/// ```
pub fn write_frame<W: Write>(w: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    // Delegates to the borrowed-payload form so there is exactly one
    // assembly implementation — output is byte-identical by construction.
    write_frame_parts(w, frame.message_type, &frame.correlation_id, &frame.payload)
}

/// Writes a frame from **borrowed** parts — the allocation-free send path
/// (PROTO-3). Identical wire bytes to [`write_frame`] (which now forwards
/// here), but the caller need not own a [`Frame`]: a sender that already
/// holds the payload as a slice (e.g. `host_api_version.as_bytes()`) avoids
/// the `Vec<u8>` copy that constructing a one-shot `Frame` forced.
/// `Frame` / [`Frame::new`] / [`read_frame`] are unchanged — this is a
/// purely additive second codec constructor, the same shape as the widgets'
/// `from_slice` borrowed constructors.
///
/// # Errors
///
/// Identical to [`write_frame`]: [`ProtocolError::FrameTooLarge`] when
/// `1 + 16 + payload.len()` exceeds [`MAX_FRAME_SIZE`] (nothing written),
/// [`ProtocolError::Io`] on a failed write/flush.
pub fn write_frame_parts<W: Write>(
    w: &mut W,
    message_type: MessageType,
    correlation_id: &[u8; 16],
    payload: &[u8],
) -> Result<(), ProtocolError> {
    // Total bytes that the length field counts: type (1) + id (16) + payload.
    let body_len = 1usize + 16 + payload.len();

    if body_len > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge {
            len: body_len as u64,
        });
    }

    // Assemble the complete frame in a heap buffer so we can commit it in one
    // write_all call — no partial frames on the pipe.
    let total = 4 + body_len;
    let mut buf = Vec::with_capacity(total);

    // 4-byte big-endian length (excludes itself).
    let len_u32 = body_len as u32;
    buf.extend_from_slice(&len_u32.to_be_bytes());

    // 1-byte message type.
    buf.push(message_type.to_byte());

    // 16-byte correlation id.
    buf.extend_from_slice(correlation_id);

    // Payload.
    buf.extend_from_slice(payload);

    debug_assert_eq!(buf.len(), total);

    // Single atomic write — the peer sees either the full frame or nothing.
    w.write_all(&buf)?;
    w.flush()?;

    Ok(())
}

/// Reads exactly one frame from `r`.
///
/// Reads the 4-byte length header and validates it (only after the cap check),
/// reads the fixed 17-byte `type + correlation-id` header onto the stack, then
/// allocates the payload buffer exactly once at its true length and reads it
/// in place — no zero-fill of an oversized buffer and no second copy.
///
/// **Any error returned here is terminal**: the caller must close the plugin
/// connection (ADR 0007 §4, "no skip-and-continue").  The codec cannot
/// resynchronise after a framing error because the stream position is
/// undefined once a frame is partially or incorrectly read.
///
/// # Errors
///
/// - [`ProtocolError::FrameTooLarge`] — the length header exceeds
///   [`MAX_FRAME_SIZE`].  The body is **not read**; no large allocation is
///   attempted.
/// - [`ProtocolError::FrameTooSmall`] — the length header is less than 17
///   (too small to hold the mandatory type byte and 16-byte id).
/// - [`ProtocolError::UnknownMessageType`] — the type byte is not a known
///   [`MessageType`] code.
/// - [`ProtocolError::Truncated`] — the stream ended before the full frame
///   body was received.
/// - [`ProtocolError::Io`] — an underlying IO error occurred.
///
/// # Example
///
/// ```
/// use std::io::Cursor;
/// use rstui_plugin_host::protocol::{Frame, MessageType, read_frame, write_frame};
///
/// let frame = Frame::new(MessageType::Ready, [0xABu8; 16], b"ok".to_vec());
/// let mut buf = Vec::new();
/// write_frame(&mut buf, &frame).unwrap();
///
/// let decoded = read_frame(&mut Cursor::new(buf)).unwrap();
/// assert_eq!(decoded.message_type, MessageType::Ready);
/// assert_eq!(decoded.correlation_id, [0xABu8; 16]);
/// assert_eq!(decoded.payload, b"ok");
/// ```
pub fn read_frame<R: Read>(r: &mut R) -> Result<Frame, ProtocolError> {
    // ── Step 1: read and validate the 4-byte length header ─────────────────
    let mut len_buf = [0u8; 4];
    read_exact_or_truncated(r, &mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);

    // Reject oversized frames before any allocation.
    if len as usize > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge { len: len as u64 });
    }

    // Reject frames too small to contain type + id.
    if len < MIN_FRAME_LEN {
        return Err(ProtocolError::FrameTooSmall { len });
    }

    // ── Step 2: read the fixed 17-byte header (type + id) onto the stack ────
    // The body is `type(1) + id(16) + payload`; `MIN_FRAME_LEN` (checked
    // above) guarantees at least the 17 header bytes are present. Reading the
    // header into a stack array — instead of a heap `vec![0u8; len]` that is
    // first zero-filled and then re-copied — lets the payload `Vec` be
    // allocated exactly once, at its true length, and read straight into.
    let mut header = [0u8; 17];
    read_exact_or_truncated(r, &mut header)?;

    // header[0] = type
    let message_type = MessageType::from_byte(header[0])?;

    // header[1..17] = correlation id
    let mut correlation_id = [0u8; 16];
    correlation_id.copy_from_slice(&header[1..17]);

    // ── Step 3: allocate the payload exactly once and read it in place ──────
    // `len >= MIN_FRAME_LEN` (17), so this subtraction cannot underflow.
    let payload_len = len as usize - 17;
    let mut payload = vec![0u8; payload_len];
    read_exact_or_truncated(r, &mut payload)?;

    Ok(Frame {
        message_type,
        correlation_id,
        payload,
    })
}

/// Reads exactly `buf.len()` bytes from `r`, mapping an early EOF to
/// [`ProtocolError::Truncated`] rather than an `UnexpectedEof` IO error.
///
/// This keeps caller error-handling exhaustive: `Truncated` means "the stream
/// ended mid-frame", which is distinct from an OS-level IO failure, and callers
/// can report it as such.
fn read_exact_or_truncated<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<(), ProtocolError> {
    match r.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Err(ProtocolError::Truncated),
        Err(e) => Err(ProtocolError::Io(e)),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// ProtocolError
// ──────────────────────────────────────────────────────────────────────────────

/// A framing or IO error from the host↔plugin codec.
///
/// Every variant signals a terminal condition: the caller must close the plugin
/// connection after receiving any of these (ADR 0007 §4, "no skip-and-continue").
///
/// `io::Error` does not implement `PartialEq`, so this type does not derive
/// `PartialEq` either.  Tests match on variant kind via pattern matching or
/// the `is_*` helper methods below.
#[derive(Debug)]
pub enum ProtocolError {
    /// An underlying IO read or write failed.
    Io(io::Error),
    /// The frame's named length exceeds [`MAX_FRAME_SIZE`].  `len` is the
    /// value from the wire; no body bytes were read or allocated.
    FrameTooLarge {
        /// The length the frame header claimed.
        len: u64,
    },
    /// The frame's named length is less than 17 — too small to contain the
    /// mandatory type byte and 16-byte correlation id.
    FrameTooSmall {
        /// The length the frame header claimed.
        len: u32,
    },
    /// The type byte is not a recognised [`MessageType`] code.  Per the
    /// fail-closed rule, unknown codes are rejected rather than defaulted.
    UnknownMessageType(u8),
    /// The stream ended before the full frame body was received.  This happens
    /// when a plugin process exits or crashes mid-transmission.
    Truncated,
}

impl ProtocolError {
    /// Returns `true` if this is a [`ProtocolError::FrameTooLarge`] error.
    #[must_use]
    pub fn is_frame_too_large(&self) -> bool {
        matches!(self, Self::FrameTooLarge { .. })
    }

    /// Returns `true` if this is a [`ProtocolError::FrameTooSmall`] error.
    #[must_use]
    pub fn is_frame_too_small(&self) -> bool {
        matches!(self, Self::FrameTooSmall { .. })
    }

    /// Returns `true` if this is a [`ProtocolError::UnknownMessageType`]
    /// error.
    #[must_use]
    pub fn is_unknown_message_type(&self) -> bool {
        matches!(self, Self::UnknownMessageType(_))
    }

    /// Returns `true` if this is a [`ProtocolError::Truncated`] error.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        matches!(self, Self::Truncated)
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::FrameTooLarge { len } => write!(
                f,
                "frame too large: length {len} exceeds MAX_FRAME_SIZE ({MAX_FRAME_SIZE})",
            ),
            Self::FrameTooSmall { len } => write!(
                f,
                "frame too small: length {len} is less than the minimum 17 \
                 (1-byte type + 16-byte correlation id)"
            ),
            Self::UnknownMessageType(b) => {
                write!(f, "unknown message type byte: 0x{b:02X}")
            }
            Self::Truncated => {
                write!(f, "stream ended before the full frame body was received")
            }
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Round-trip `frame` through `write_frame` → `read_frame` over a Vec<u8>.
    fn round_trip(frame: &Frame) -> Frame {
        let mut buf = Vec::new();
        write_frame(&mut buf, frame).expect("encode failed");
        let mut cursor = Cursor::new(buf);
        read_frame(&mut cursor).expect("decode failed")
    }

    // ── MessageType from_byte / to_byte ──────────────────────────────────────

    #[test]
    fn from_byte_accepts_all_assigned_codes() {
        let cases: &[(u8, MessageType)] = &[
            (0x01, MessageType::Initialize),
            (0x02, MessageType::HookDispatch),
            (0x03, MessageType::CapabilityResponse),
            (0x04, MessageType::Shutdown),
            (0x81, MessageType::Ready),
            (0x82, MessageType::CapabilityCall),
            (0x83, MessageType::HookResult),
            (0x84, MessageType::Log),
        ];
        for &(byte, expected) in cases {
            let got = MessageType::from_byte(byte).expect("should be valid");
            assert_eq!(got, expected, "from_byte(0x{byte:02X})");
            assert_eq!(got.to_byte(), byte, "to_byte() round-trip for 0x{byte:02X}");
        }
    }

    #[test]
    fn from_byte_rejects_unassigned_codes() {
        // Sample of unassigned bytes across the full range.
        for b in [0x00u8, 0x05, 0x7F, 0x80, 0x85, 0xFF] {
            assert!(
                MessageType::from_byte(b).is_err(),
                "byte 0x{b:02X} should be unknown"
            );
            assert!(
                MessageType::from_byte(b)
                    .unwrap_err()
                    .is_unknown_message_type(),
                "wrong error kind for 0x{b:02X}"
            );
        }
    }

    #[test]
    fn from_host_and_from_plugin_classify_all_variants_correctly() {
        let host_variants = [
            MessageType::Initialize,
            MessageType::HookDispatch,
            MessageType::CapabilityResponse,
            MessageType::Shutdown,
        ];
        let plugin_variants = [
            MessageType::Ready,
            MessageType::CapabilityCall,
            MessageType::HookResult,
            MessageType::Log,
        ];

        for v in host_variants {
            assert!(v.from_host(), "{v:?} should be from_host");
            assert!(!v.from_plugin(), "{v:?} should not be from_plugin");
        }
        for v in plugin_variants {
            assert!(v.from_plugin(), "{v:?} should be from_plugin");
            assert!(!v.from_host(), "{v:?} should not be from_host");
        }
    }

    // ── round-trip: every MessageType with empty payload ─────────────────────

    #[test]
    fn round_trip_every_message_type_with_empty_payload() {
        let all_types = [
            MessageType::Initialize,
            MessageType::HookDispatch,
            MessageType::CapabilityResponse,
            MessageType::Shutdown,
            MessageType::Ready,
            MessageType::CapabilityCall,
            MessageType::HookResult,
            MessageType::Log,
        ];
        for mt in all_types {
            let frame = Frame::new(mt, [0u8; 16], vec![]);
            let decoded = round_trip(&frame);
            assert_eq!(decoded.message_type, mt);
            assert_eq!(decoded.correlation_id, [0u8; 16]);
            assert!(decoded.payload.is_empty());
        }
    }

    // ── round-trip: non-trivial payload ──────────────────────────────────────

    #[test]
    fn round_trip_with_payload() {
        let payload: Vec<u8> = (0u8..=255).collect();
        let id = [
            0xDE, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
        ];
        let frame = Frame::new(MessageType::HookDispatch, id, payload.clone());
        let decoded = round_trip(&frame);
        assert_eq!(decoded.message_type, MessageType::HookDispatch);
        assert_eq!(decoded.correlation_id, id);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn round_trip_large_but_legal_payload() {
        // MAX_FRAME_SIZE - 17 is the largest legal payload.
        let payload = vec![0xABu8; MAX_FRAME_SIZE - 17];
        let frame = Frame::new(MessageType::CapabilityResponse, [7u8; 16], payload.clone());
        let decoded = round_trip(&frame);
        assert_eq!(decoded.payload.len(), MAX_FRAME_SIZE - 17);
        assert_eq!(decoded.payload, payload);
    }

    // ── round-trip: correlation id survives byte-exact ────────────────────────

    #[test]
    fn correlation_id_round_trips_byte_exact() {
        let id: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        let frame = Frame::new(MessageType::Log, id, b"test".to_vec());
        let decoded = round_trip(&frame);
        assert_eq!(decoded.correlation_id, id);
    }

    // ── stream framing: multiple frames back-to-back ──────────────────────────

    #[test]
    fn multiple_frames_written_back_to_back_are_read_in_order() {
        let frames = vec![
            Frame::new(MessageType::Initialize, [1u8; 16], b"frame-1".to_vec()),
            Frame::new(MessageType::HookDispatch, [2u8; 16], vec![]),
            Frame::new(MessageType::Shutdown, [3u8; 16], b"last".to_vec()),
        ];

        let mut buf = Vec::new();
        for f in &frames {
            write_frame(&mut buf, f).expect("encode failed");
        }

        let mut cursor = Cursor::new(buf);
        for expected in &frames {
            let got = read_frame(&mut cursor).expect("decode failed");
            assert_eq!(got.message_type, expected.message_type);
            assert_eq!(got.correlation_id, expected.correlation_id);
            assert_eq!(got.payload, expected.payload);
        }
    }

    // ── encode: oversize payload ⇒ FrameTooLarge, nothing written ────────────

    #[test]
    fn encode_oversize_payload_returns_frame_too_large_and_writes_nothing() {
        // One byte more than the maximum payload (which is MAX_FRAME_SIZE - 17).
        let payload = vec![0u8; MAX_FRAME_SIZE - 17 + 1];
        let frame = Frame::new(MessageType::Ready, [0u8; 16], payload);

        let mut buf: Vec<u8> = Vec::new();
        let err = write_frame(&mut buf, &frame).expect_err("should fail");

        assert!(
            err.is_frame_too_large(),
            "expected FrameTooLarge, got: {err}"
        );
        assert!(
            buf.is_empty(),
            "nothing must be written on a too-large frame"
        );
    }

    // ── decode: oversize length header ⇒ FrameTooLarge, body not read ────────

    #[test]
    fn decode_oversize_length_header_returns_frame_too_large_without_reading_body() {
        // Craft a 4-byte header claiming MAX_FRAME_SIZE + 1 bytes.
        let claimed = (MAX_FRAME_SIZE + 1) as u32;
        let header = claimed.to_be_bytes();

        // Feed only the header — no body bytes at all.  If the implementation
        // tried to read the body it would hit UnexpectedEof, not FrameTooLarge.
        let mut cursor = Cursor::new(header.to_vec());
        let err = read_frame(&mut cursor).expect_err("should fail");
        assert!(
            err.is_frame_too_large(),
            "expected FrameTooLarge, got: {err}"
        );
    }

    // ── decode: length < 17 ⇒ FrameTooSmall ─────────────────────────────────

    #[test]
    fn decode_length_below_minimum_returns_frame_too_small() {
        for &short_len in &[0u32, 1, 15, 16] {
            let mut buf = Vec::new();
            // Write a body of `short_len` bytes so we don't hit Truncated.
            buf.extend_from_slice(&short_len.to_be_bytes());
            buf.extend(vec![0u8; short_len as usize]);

            let err = read_frame(&mut Cursor::new(buf)).expect_err("should fail");
            assert!(
                err.is_frame_too_small(),
                "len={short_len}: expected FrameTooSmall, got: {err}"
            );
        }
    }

    // ── decode: truncated body ⇒ Truncated ───────────────────────────────────

    #[test]
    fn decode_truncated_body_returns_truncated() {
        // Claim 20 bytes (17 min + 3 payload) but only provide 10.
        let claimed: u32 = 20;
        let mut buf = claimed.to_be_bytes().to_vec();
        buf.extend(vec![0u8; 10]); // only 10 of the claimed 20 body bytes

        let err = read_frame(&mut Cursor::new(buf)).expect_err("should fail");
        assert!(err.is_truncated(), "expected Truncated, got: {err}");
    }

    #[test]
    fn decode_truncated_header_returns_truncated() {
        // Provide only 3 bytes of the 4-byte header.
        let buf = vec![0x00u8, 0x00, 0x00];
        let err = read_frame(&mut Cursor::new(buf)).expect_err("should fail");
        assert!(err.is_truncated(), "expected Truncated, got: {err}");
    }

    // ── decode: unknown type byte ⇒ UnknownMessageType ───────────────────────

    #[test]
    fn decode_unknown_type_byte_returns_unknown_message_type() {
        // Craft a minimal valid-length frame (17 bytes body) with type 0x00.
        let len: u32 = 17;
        let mut buf = len.to_be_bytes().to_vec();
        buf.push(0x00); // unknown type
        buf.extend([0u8; 16]); // correlation id

        let err = read_frame(&mut Cursor::new(buf)).expect_err("should fail");
        assert!(
            err.is_unknown_message_type(),
            "expected UnknownMessageType, got: {err}"
        );
    }

    // ── ProtocolError display ─────────────────────────────────────────────────

    #[test]
    fn protocol_error_display_is_informative() {
        let s = ProtocolError::FrameTooLarge { len: 99 }.to_string();
        assert!(
            s.contains("99"),
            "display should include the claimed length"
        );

        let s = ProtocolError::FrameTooSmall { len: 5 }.to_string();
        assert!(s.contains('5'), "display should include the claimed length");

        let s = ProtocolError::UnknownMessageType(0xAB).to_string();
        assert!(
            s.to_lowercase().contains("ab"),
            "display should include the byte in hex"
        );

        let s = ProtocolError::Truncated.to_string();
        assert!(!s.is_empty());
    }

    // ── From<io::Error> ───────────────────────────────────────────────────────

    #[test]
    fn from_io_error_wraps_correctly() {
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed");
        let proto_err = ProtocolError::from(io_err);
        assert!(
            matches!(proto_err, ProtocolError::Io(_)),
            "should wrap as Io variant"
        );
        // std::error::Error::source should return the inner io::Error.
        use std::error::Error;
        assert!(
            proto_err.source().is_some(),
            "source() should return Some for Io variant"
        );
    }

    // ── wire layout sanity: byte-level inspection ─────────────────────────────

    #[test]
    fn wire_bytes_match_specified_layout() {
        // Build a frame with a known payload and verify the raw bytes.
        let id = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10u8,
        ];
        let payload = vec![0xAAu8, 0xBB, 0xCC];
        let frame = Frame::new(MessageType::Initialize, id, payload.clone());

        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();

        // Length field: 1 (type) + 16 (id) + 3 (payload) = 20
        assert_eq!(&buf[0..4], &20u32.to_be_bytes(), "length field");
        // Type byte
        assert_eq!(buf[4], 0x01, "Initialize type byte");
        // Correlation id
        assert_eq!(&buf[5..21], &id, "correlation id");
        // Payload
        assert_eq!(&buf[21..], &payload[..], "payload");
        // Total length
        assert_eq!(buf.len(), 4 + 20);
    }

    // ── PROTO-3: the borrowed-parts writer is byte-identical ──────────────────

    #[test]
    fn write_frame_parts_is_byte_identical_to_write_frame() {
        // The allocation-free send path must put exactly the same bytes on
        // the wire as `write_frame(&Frame)` for every message type and a
        // range of payloads — pins the shared assembly so a future change
        // to one path cannot silently diverge from the other.
        let ids = [[0u8; 16], [0xABu8; 16]];
        let payloads: [&[u8]; 4] = [b"", b"x", b"hook-dispatch-bytes", &[0u8, 255, 1, 254]];
        let all_types = [
            MessageType::Initialize,
            MessageType::HookDispatch,
            MessageType::CapabilityResponse,
            MessageType::Shutdown,
            MessageType::Ready,
            MessageType::CapabilityCall,
            MessageType::HookResult,
            MessageType::Log,
        ];
        for mt in all_types {
            for id in &ids {
                for p in &payloads {
                    let mut via_frame = Vec::new();
                    write_frame(&mut via_frame, &Frame::new(mt, *id, p.to_vec())).unwrap();
                    let mut via_parts = Vec::new();
                    write_frame_parts(&mut via_parts, mt, id, p).unwrap();
                    assert_eq!(
                        via_frame,
                        via_parts,
                        "wire bytes diverged for {mt:?} id={id:?} payload_len={}",
                        p.len()
                    );
                }
            }
        }
    }
}
