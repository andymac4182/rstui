//! The plugin *author's* side of the protocol: [`PluginConnection`], the
//! counterpart to [`host::PluginHost`](crate::host::PluginHost).
//!
//! The host half (spawn, mediate, enforce) is the security boundary. This
//! is the other half: the few lines a plugin binary needs so it never
//! hand-rolls framing. A real plugin's `main` is essentially:
//!
//! ```no_run
//! use std::io::{stdin, stdout};
//! use rstui_plugin_host::capability::CapabilityRequest;
//! use rstui_plugin_host::plugin_sdk::PluginConnection;
//!
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mut host = PluginConnection::connect(stdin().lock(), stdout().lock())?;
//! host.log("starting up")?;
//! let response = host.request(&CapabilityRequest::Env { key: "PATH".into() })?;
//! // `response` is Ok{payload} if granted, Denied{reason} if the policy
//! // refused, or Failed{error} if the host effect itself failed.
//! let _ = response;
//! # Ok(()) }
//! ```
//!
//! It is generic over the reader/writer so the same code runs over a real
//! process's stdin/stdout *and* over in-memory pipes in a deterministic
//! test — no real process, the `Harness` standard (ADR 0007 §5). Both
//! halves share [`crate::protocol`] and [`crate::message`], so a frame the
//! SDK writes is by construction exactly what the host decodes; the SDK
//! adds no new wire behaviour, only the plugin-side ergonomics (handshake,
//! correlation-id bookkeeping, typed request/response).

use std::fmt;
use std::io::{Read, Write};

use crate::capability::CapabilityRequest;
use crate::hook::{HookKind, HookOutcome, HookReduction};
use crate::message::{
    CapabilityResponse, MessageError, decode_hook_dispatch, decode_response, encode_hook_result,
    encode_request,
};
use crate::protocol::{Frame, MessageType, ProtocolError, read_frame, write_frame};

/// A failure on the plugin side of the protocol.
#[derive(Debug)]
pub enum SdkError {
    /// A framing error reading from or writing to the host (the host
    /// closed the pipe, a frame was malformed, …).
    Protocol(ProtocolError),
    /// A [`CapabilityResponse`] payload could not be decoded.
    Message(MessageError),
    /// The host sent a frame the plugin did not expect at this point —
    /// e.g. something other than `Initialize` during the handshake, or
    /// other than a correlation-matched `CapabilityResponse` after a call.
    UnexpectedFrame {
        /// What the plugin was waiting for.
        expected: &'static str,
        /// What the host actually sent.
        got: MessageType,
    },
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(e) => write!(f, "plugin protocol error: {e}"),
            Self::Message(e) => write!(f, "plugin message decode error: {e}"),
            Self::UnexpectedFrame { expected, got } => {
                write!(f, "unexpected frame: expected {expected}, got {got:?}")
            }
        }
    }
}

impl std::error::Error for SdkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Protocol(e) => Some(e),
            Self::Message(e) => Some(e),
            Self::UnexpectedFrame { .. } => None,
        }
    }
}

impl From<ProtocolError> for SdkError {
    fn from(e: ProtocolError) -> Self {
        Self::Protocol(e)
    }
}

impl From<MessageError> for SdkError {
    fn from(e: MessageError) -> Self {
        Self::Message(e)
    }
}

/// The plugin's connection to its host: a thin, correct client over
/// [`crate::protocol`] + [`crate::message`].
///
/// Construct it with [`connect`](PluginConnection::connect), which performs
/// the plugin side of the `Initialize` → `Ready` handshake. Then call
/// [`request`](PluginConnection::request) for each capability the plugin
/// needs and [`log`](PluginConnection::log) for diagnostics.
pub struct PluginConnection<R: Read, W: Write> {
    reader: R,
    writer: W,
    host_api_version: String,
    next_id: u64,
    /// Invoked when the host dispatches a hook (ADR 0007 §6). The default
    /// returns [`HookOutcome::Continue`] for everything, so a plugin that
    /// does not care about hooks is unaffected — [`request`] transparently
    /// services dispatches. Replace it with [`set_hook_handler`] to veto.
    ///
    /// [`request`]: PluginConnection::request
    /// [`set_hook_handler`]: PluginConnection::set_hook_handler
    hook_handler: HookHandler,
}

/// A plugin's hook callback: given the [`HookKind`] and its input bytes,
/// return the [`HookOutcome`]. Boxed so it can be swapped at runtime;
/// `Send` so a `PluginConnection` can move across threads.
type HookHandler = Box<dyn FnMut(HookKind, &[u8]) -> HookOutcome + Send>;

impl<R: Read, W: Write> fmt::Debug for PluginConnection<R, W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginConnection")
            .field("host_api_version", &self.host_api_version)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl<R: Read, W: Write> PluginConnection<R, W> {
    /// Perform the plugin side of the handshake: read the host's
    /// `Initialize` frame (its payload is the host's `api_version`), then
    /// reply `Ready`.
    ///
    /// # Errors
    ///
    /// [`SdkError::Protocol`] if the host stream is unreadable/closed,
    /// [`SdkError::UnexpectedFrame`] if the first frame is not
    /// `Initialize`.
    pub fn connect(reader: R, writer: W) -> Result<Self, SdkError> {
        let mut reader = reader;
        let mut writer = writer;
        let init = read_frame(&mut reader)?;
        if init.message_type != MessageType::Initialize {
            return Err(SdkError::UnexpectedFrame {
                expected: "Initialize",
                got: init.message_type,
            });
        }
        let host_api_version = String::from_utf8_lossy(&init.payload).into_owned();
        write_frame(
            &mut writer,
            &Frame::new(MessageType::Ready, init.correlation_id, Vec::new()),
        )?;
        Ok(Self {
            reader,
            writer,
            host_api_version,
            next_id: 1,
            hook_handler: Box::new(|_, _| HookOutcome::Continue),
        })
    }

    /// Replace the hook handler. It is called with the [`HookKind`] and the
    /// hook's input bytes (for `before_capability` that is an encoded
    /// [`CapabilityRequest`]) and returns the plugin's [`HookOutcome`].
    /// Only `VetoChain` hooks consult the return value; for an `Observe`
    /// hook the outcome is ignored (the host awaits no reply). A handler
    /// can only ever *narrow* — returning [`HookOutcome::Veto`] denies an
    /// already-policy-permitted call; it can never widen one (ADR 0007 §6).
    pub fn set_hook_handler(
        &mut self,
        handler: impl FnMut(HookKind, &[u8]) -> HookOutcome + Send + 'static,
    ) {
        self.hook_handler = Box::new(handler);
    }

    /// Service one inbound `HookDispatch` frame: decode it, invoke the
    /// handler, and — only for a `VetoChain` hook — reply `HookResult`
    /// echoing the dispatch's correlation id. `Observe` hooks are one-way.
    fn service_hook(&mut self, frame: &Frame) -> Result<(), SdkError> {
        let (kind, input) = decode_hook_dispatch(&frame.payload)?;
        let outcome = (self.hook_handler)(kind, &input);
        if matches!(kind.reduction(), HookReduction::VetoChain) {
            write_frame(
                &mut self.writer,
                &Frame::new(
                    MessageType::HookResult,
                    frame.correlation_id,
                    encode_hook_result(&outcome),
                ),
            )?;
        }
        Ok(())
    }

    /// The host-protocol version the host announced in its `Initialize`
    /// frame (the plugin can refuse to proceed if it cannot satisfy it).
    #[must_use]
    pub fn host_api_version(&self) -> &str {
        &self.host_api_version
    }

    /// Ask the host to perform `request`, blocking until the matching
    /// [`CapabilityResponse`] arrives.
    ///
    /// The returned response is the host's verdict: `Ok` (granted, with
    /// the effect result), `Denied` (the policy refused — with the
    /// reason), or `Failed` (granted but the effect errored). A denial is
    /// a *value*, not an `Err`: being refused is a normal, expected
    /// outcome a well-behaved plugin handles.
    ///
    /// # Errors
    ///
    /// [`SdkError`] only for *protocol* faults — the host stream closed, a
    /// non-response or correlation-mismatched frame arrived, or the
    /// response payload was undecodable.
    pub fn request(&mut self, request: &CapabilityRequest) -> Result<CapabilityResponse, SdkError> {
        let id = self.fresh_id();
        write_frame(
            &mut self.writer,
            &Frame::new(MessageType::CapabilityCall, id, encode_request(request)),
        )?;
        // The host may interleave a host-initiated `HookDispatch` (e.g.
        // `before_capability` for *this* call) ahead of the response.
        // Service every hook transparently, then return the matching
        // `CapabilityResponse` (ADR 0007 §6).
        loop {
            let frame = read_frame(&mut self.reader)?;
            match frame.message_type {
                MessageType::HookDispatch => {
                    self.service_hook(&frame)?;
                }
                MessageType::CapabilityResponse => {
                    if frame.correlation_id != id {
                        return Err(SdkError::UnexpectedFrame {
                            expected: "CapabilityResponse with the matching correlation id",
                            got: frame.message_type,
                        });
                    }
                    return Ok(decode_response(&frame.payload)?);
                }
                got => {
                    return Err(SdkError::UnexpectedFrame {
                        expected: "CapabilityResponse",
                        got,
                    });
                }
            }
        }
    }

    /// Send a diagnostic log line to the host (delivered as a `Log`
    /// frame; the host collects these into the run report's `logs`).
    ///
    /// # Errors
    ///
    /// [`SdkError::Protocol`] if the line could not be written.
    pub fn log(&mut self, line: &str) -> Result<(), SdkError> {
        write_frame(
            &mut self.writer,
            &Frame::new(MessageType::Log, [0u8; 16], line.as_bytes().to_vec()),
        )?;
        Ok(())
    }

    /// A fresh 16-byte correlation id (counter in the first 8 bytes,
    /// big-endian) so the plugin can match a response to its call.
    fn fresh_id(&mut self) -> [u8; 16] {
        let n = self.next_id;
        self.next_id += 1;
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&n.to_be_bytes());
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::FsMode;
    use std::io::Cursor;

    /// Encode a host→plugin frame stream the SDK will read.
    fn host_says(frames: &[Frame]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in frames {
            write_frame(&mut out, frame).unwrap();
        }
        out
    }

    fn init() -> Frame {
        Frame::new(MessageType::Initialize, [0u8; 16], b"1".to_vec())
    }

    #[test]
    fn connect_reads_initialize_and_replies_ready() {
        let inbound = host_says(&[init()]);
        let mut outbound = Vec::new();
        let conn = PluginConnection::connect(Cursor::new(inbound), &mut outbound).unwrap();
        assert_eq!(conn.host_api_version(), "1");

        // The plugin's first emitted frame is Ready.
        let sent = read_frame(&mut Cursor::new(outbound)).unwrap();
        assert_eq!(sent.message_type, MessageType::Ready);
    }

    #[test]
    fn connect_rejects_a_non_initialize_first_frame() {
        let inbound = host_says(&[Frame::new(MessageType::Shutdown, [0u8; 16], Vec::new())]);
        let err = PluginConnection::connect(Cursor::new(inbound), Vec::new()).unwrap_err();
        assert!(matches!(
            err,
            SdkError::UnexpectedFrame {
                expected: "Initialize",
                got: MessageType::Shutdown
            }
        ));
    }

    #[test]
    fn request_writes_a_capability_call_and_parses_the_response() {
        let req = CapabilityRequest::Filesystem {
            mode: FsMode::Read,
            path: "/data/x".into(),
            contents: Vec::new(),
        };
        // The host will reply on the same correlation id the SDK chooses
        // for its first request: counter 1.
        let mut response_id = [0u8; 16];
        response_id[7] = 1;
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::CapabilityResponse,
                response_id,
                crate::message::encode_response(&CapabilityResponse::Ok {
                    payload: b"file-bytes".to_vec(),
                }),
            ),
        ]);
        let mut outbound = Vec::new();
        let mut conn = PluginConnection::connect(Cursor::new(inbound), &mut outbound).unwrap();

        let response = conn.request(&req).unwrap();
        assert_eq!(
            response,
            CapabilityResponse::Ok {
                payload: b"file-bytes".to_vec()
            }
        );

        // What the plugin actually put on the wire: Ready, then a
        // CapabilityCall whose payload is exactly encode_request(&req).
        let mut cursor = Cursor::new(outbound);
        let ready = read_frame(&mut cursor).unwrap();
        assert_eq!(ready.message_type, MessageType::Ready);
        let call = read_frame(&mut cursor).unwrap();
        assert_eq!(call.message_type, MessageType::CapabilityCall);
        assert_eq!(call.payload, encode_request(&req));
        assert_eq!(call.correlation_id, response_id);
    }

    #[test]
    fn request_surfaces_denied_as_a_value_not_an_error() {
        let mut id = [0u8; 16];
        id[7] = 1;
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::CapabilityResponse,
                id,
                crate::message::encode_response(&CapabilityResponse::Denied {
                    reason: "no grant".into(),
                }),
            ),
        ]);
        let mut conn = PluginConnection::connect(Cursor::new(inbound), Vec::new()).unwrap();
        let response = conn
            .request(&CapabilityRequest::Env { key: "X".into() })
            .unwrap();
        assert_eq!(
            response,
            CapabilityResponse::Denied {
                reason: "no grant".into()
            }
        );
    }

    #[test]
    fn request_errors_on_a_misdirected_response_frame() {
        let inbound = host_says(&[
            init(),
            Frame::new(MessageType::Shutdown, [0u8; 16], Vec::new()),
        ]);
        let mut conn = PluginConnection::connect(Cursor::new(inbound), Vec::new()).unwrap();
        let err = conn
            .request(&CapabilityRequest::Env { key: "X".into() })
            .unwrap_err();
        assert!(matches!(
            err,
            SdkError::UnexpectedFrame {
                expected: "CapabilityResponse",
                ..
            }
        ));
    }

    #[test]
    fn request_errors_on_correlation_id_mismatch() {
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::CapabilityResponse,
                [9u8; 16], // not the id the SDK will have used
                crate::message::encode_response(&CapabilityResponse::Ok {
                    payload: Vec::new(),
                }),
            ),
        ]);
        let mut conn = PluginConnection::connect(Cursor::new(inbound), Vec::new()).unwrap();
        let err = conn
            .request(&CapabilityRequest::Env { key: "X".into() })
            .unwrap_err();
        assert!(matches!(err, SdkError::UnexpectedFrame { .. }));
    }

    #[test]
    fn log_emits_a_log_frame() {
        let inbound = host_says(&[init()]);
        let mut outbound = Vec::new();
        let mut conn = PluginConnection::connect(Cursor::new(inbound), &mut outbound).unwrap();
        conn.log("hello host").unwrap();

        let mut cursor = Cursor::new(outbound);
        let _ready = read_frame(&mut cursor).unwrap();
        let log = read_frame(&mut cursor).unwrap();
        assert_eq!(log.message_type, MessageType::Log);
        assert_eq!(log.payload, b"hello host");
    }

    #[test]
    fn correlation_ids_increase_per_request() {
        let mut id1 = [0u8; 16];
        id1[7] = 1;
        let mut id2 = [0u8; 16];
        id2[7] = 2;
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::CapabilityResponse,
                id1,
                crate::message::encode_response(&CapabilityResponse::Ok {
                    payload: Vec::new(),
                }),
            ),
            Frame::new(
                MessageType::CapabilityResponse,
                id2,
                crate::message::encode_response(&CapabilityResponse::Ok {
                    payload: Vec::new(),
                }),
            ),
        ]);
        let mut conn = PluginConnection::connect(Cursor::new(inbound), Vec::new()).unwrap();
        let r = CapabilityRequest::Env { key: "X".into() };
        assert!(conn.request(&r).is_ok());
        assert!(conn.request(&r).is_ok(), "second call uses the next id");
    }

    /// The SDK's first `request` uses correlation id `1` (counter starts
    /// at 1, big-endian in the first 8 bytes).
    fn first_request_id() -> [u8; 16] {
        let mut id = [0u8; 16];
        id[7] = 1;
        id
    }

    #[test]
    fn request_services_a_vetochain_hook_then_returns_the_host_response() {
        // The host interleaves a `before_capability` HookDispatch ahead of
        // the CapabilityResponse. The SDK must invoke the handler, reply
        // HookResult (echoing the dispatch id), then return the response.
        let hook_corr = {
            let mut c = [0u8; 16];
            c[7] = 42;
            c
        };
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::HookDispatch,
                hook_corr,
                crate::message::encode_hook_dispatch(HookKind::BeforeCapability, b"req-bytes"),
            ),
            Frame::new(
                MessageType::CapabilityResponse,
                first_request_id(),
                crate::message::encode_response(&CapabilityResponse::Ok {
                    payload: b"R".to_vec(),
                }),
            ),
        ]);
        let mut outbound = Vec::new();
        let mut conn = PluginConnection::connect(Cursor::new(inbound), &mut outbound).unwrap();

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen2 = std::sync::Arc::clone(&seen);
        conn.set_hook_handler(move |kind, input| {
            seen2.lock().unwrap().push((kind, input.to_vec()));
            HookOutcome::Veto {
                reason: "sdk says no".into(),
            }
        });

        let resp = conn
            .request(&CapabilityRequest::Env { key: "X".into() })
            .unwrap();
        assert_eq!(
            resp,
            CapabilityResponse::Ok {
                payload: b"R".to_vec()
            }
        );
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(HookKind::BeforeCapability, b"req-bytes".to_vec())],
            "handler invoked with the dispatched kind + input"
        );

        // Outbound: Ready, CapabilityCall, then a HookResult(Veto) echoing
        // the dispatch's correlation id.
        let mut c = Cursor::new(outbound);
        assert_eq!(read_frame(&mut c).unwrap().message_type, MessageType::Ready);
        assert_eq!(
            read_frame(&mut c).unwrap().message_type,
            MessageType::CapabilityCall
        );
        let hr = read_frame(&mut c).unwrap();
        assert_eq!(hr.message_type, MessageType::HookResult);
        assert_eq!(hr.correlation_id, hook_corr, "echoes the dispatch id");
        assert_eq!(
            crate::message::decode_hook_result(&hr.payload).unwrap(),
            HookOutcome::Veto {
                reason: "sdk says no".into()
            }
        );
    }

    #[test]
    fn observe_hook_dispatch_is_serviced_without_a_reply() {
        // SessionStart is Observe: the SDK invokes the handler but must NOT
        // write a HookResult (the host awaits none).
        let inbound = host_says(&[
            init(),
            Frame::new(
                MessageType::HookDispatch,
                [0u8; 16],
                crate::message::encode_hook_dispatch(HookKind::SessionStart, &[]),
            ),
            Frame::new(
                MessageType::CapabilityResponse,
                first_request_id(),
                crate::message::encode_response(&CapabilityResponse::Ok { payload: vec![] }),
            ),
        ]);
        let mut outbound = Vec::new();
        let mut conn = PluginConnection::connect(Cursor::new(inbound), &mut outbound).unwrap();
        let _ = conn
            .request(&CapabilityRequest::Env { key: "X".into() })
            .unwrap();

        // Exactly two outbound frames: Ready then CapabilityCall — no
        // HookResult for the Observe hook.
        let mut c = Cursor::new(outbound);
        assert_eq!(read_frame(&mut c).unwrap().message_type, MessageType::Ready);
        assert_eq!(
            read_frame(&mut c).unwrap().message_type,
            MessageType::CapabilityCall
        );
        assert!(
            read_frame(&mut c).is_err(),
            "no further frame — Observe hooks are one-way"
        );
    }
}
