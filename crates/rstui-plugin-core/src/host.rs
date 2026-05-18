//! The app-agnostic plugin host (ADR 0021): a [`Protocol`] trait plus a
//! serve loop and every transport selector, all **generic over the
//! protocol**. An application defines its own `Event`/`Action` vocabulary
//! and one `Protocol` impl; it then gets stdio / length-prefixed / Unix-
//! socket / WebSocket / shared-memory transports and `--shm/--uds/--ws/
//! --lp` auto-selection for free. No application vocabulary lives here.

use crate::jsonrpc::{Kind, Message};
use crate::transport::{LpTransport, StdioTransport, Transport};

/// Maps an application's plugin vocabulary onto JSON-RPC [`Message`]s.
///
/// Implement this once per app. The serve loop answers the `initialize`
/// request automatically (a JSON-RPC handshake convention shared by
/// LSP/MCP/ACP) using [`initialize_ack`](Self::initialize_ack); every
/// other message is routed through [`decode_event`](Self::decode_event)
/// and [`encode_action`](Self::encode_action).
pub trait Protocol {
    /// Host → plugin events the handler reacts to.
    type Event;
    /// Plugin → host actions the handler emits.
    type Action;

    /// JSON-RPC `result` payload for the `initialize` request, or `None`
    /// to not auto-acknowledge (the app does its own handshake).
    fn initialize_ack(&self) -> Option<serde_json::Value>;

    /// Recover an [`Event`](Self::Event) from an inbound message, or
    /// `None` to ignore it (responses, unknown methods — never fatal).
    fn decode_event(&self, msg: &Message) -> Option<Self::Event>;

    /// Frame an [`Action`](Self::Action) as an outbound message.
    fn encode_action(&self, action: &Self::Action) -> Message;

    /// `true` when this event means "drain, stop the loop, and exit".
    fn is_shutdown(&self, event: &Self::Event) -> bool;
}

/// Runs a plugin over `transport`, decoding/encoding via `proto`, until
/// end-of-stream or a shutdown event. The `initialize` request is
/// answered automatically (with [`Protocol::initialize_ack`]) before its
/// event is delivered to `handler`.
// `proto` is taken by value on purpose: the serve loop *owns* the
// protocol for its whole lifetime (symmetric with the consumed
// `transport`, and so a future stateful `Protocol` is supported). It
// only happens to use `&self` methods today, so silence the by-ref
// suggestion — `&P` would force every caller/selector to juggle a borrow.
#[allow(clippy::needless_pass_by_value)]
pub fn serve_over<T, P, F>(mut transport: T, proto: P, mut handler: F)
where
    T: Transport,
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    while let Ok(Some(msg)) = transport.recv() {
        if msg.kind() == Kind::Response {
            continue; // not addressed to a plugin
        }
        if msg.kind() == Kind::Request && msg.method.as_deref() == Some("initialize") {
            if let (Some(id), Some(ack)) = (msg.id.clone(), proto.initialize_ack()) {
                let _ = transport.send(&Message::response(id, ack));
            }
        }
        let Some(event) = proto.decode_event(&msg) else {
            continue;
        };
        let stop = proto.is_shutdown(&event);

        let mut outbox: Vec<P::Action> = Vec::new();
        {
            let mut emit = |a: P::Action| outbox.push(a);
            handler(event, &mut emit);
        }
        for action in &outbox {
            if transport.send(&proto.encode_action(action)).is_err() {
                return;
            }
        }
        if stop {
            return;
        }
    }
}

/// Serve over the default stdio transport (newline-delimited JSON-RPC).
pub fn serve<P, F>(proto: P, handler: F)
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    serve_over(StdioTransport::new(), proto, handler);
}

/// Serve over stdio with length-prefixed binary framing (no newline scan).
pub fn serve_stdio_lp<P, F>(proto: P, handler: F)
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    serve_over(
        LpTransport::new(std::io::stdin(), std::io::stdout()),
        proto,
        handler,
    );
}

/// Serve as a one-shot Unix-domain-socket server. `lp` selects
/// length-prefixed framing; otherwise newline JSON.
///
/// # Errors
///
/// Bind/accept failures.
#[cfg(unix)]
pub fn serve_unix<P, F>(path: &str, lp: bool, proto: P, handler: F) -> std::io::Result<()>
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;

    let _ = std::fs::remove_file(path); // bind fails if the path exists
    let listener = UnixListener::bind(path)?;
    let (stream, _) = listener.accept()?;
    let _ = std::fs::remove_file(path); // unlink once bound (one-shot)
    if lp {
        serve_over(
            LpTransport::new(stream.try_clone()?, stream),
            proto,
            handler,
        );
    } else {
        let read = BufReader::new(stream.try_clone()?);
        serve_over(
            crate::transport::IoTransport::new(read, stream),
            proto,
            handler,
        );
    }
    Ok(())
}

/// No `AF_UNIX` on this target — fall back to stdio so the plugin runs.
///
/// # Errors
///
/// Never (stdio fallback); the `Result` matches the Unix signature.
#[cfg(not(unix))]
pub fn serve_unix<P, F>(_path: &str, _lp: bool, proto: P, handler: F) -> std::io::Result<()>
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    serve(proto, handler);
    Ok(())
}

/// Serve as a WebSocket server: bind `addr`, accept one client.
///
/// # Errors
///
/// Bind/accept/handshake failures.
pub fn serve_ws<P, F>(
    addr: impl std::net::ToSocketAddrs,
    proto: P,
    handler: F,
) -> std::io::Result<()>
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    let transport = crate::ws::WsTransport::accept(addr)?;
    serve_over(transport, proto, handler);
    Ok(())
}

/// Serve over a shared-memory channel (ADR 0016): attach to the segment
/// at `path` (the host created it) and dispatch over an
/// [`ShmTransport`](crate::transport::ShmTransport).
///
/// # Errors
///
/// Segment attach (`mmap` / semaphore) failure.
pub fn serve_shm<P, F>(path: &str, proto: P, handler: F) -> std::io::Result<()>
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    let chan = rstui_acp_shm::ShmChannel::open(path)?;
    serve_over(crate::transport::ShmTransport::new(chan), proto, handler);
    Ok(())
}

/// The transport-selecting entry: a `--shm <path>`, `--uds <path>`, or
/// `--ws <port>` CLI arg (or the matching `RSTUI_PLUGIN_SHM`/`_UDS`/`_WS`
/// env var) picks shared memory, a Unix socket, or a WebSocket; `--lp` /
/// `RSTUI_PLUGIN_LP` selects length-prefixed framing for the uds/stdio
/// paths; otherwise newline stdio. One binary, every transport.
pub fn serve_auto<P, F>(proto: P, handler: F)
where
    P: Protocol,
    F: FnMut(P::Event, &mut dyn FnMut(P::Action)),
{
    let args: Vec<String> = std::env::args().collect();
    let arg_val = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let shm: Option<String> = arg_val("--shm").or_else(|| std::env::var("RSTUI_PLUGIN_SHM").ok());
    let uds: Option<String> = arg_val("--uds").or_else(|| std::env::var("RSTUI_PLUGIN_UDS").ok());
    let ws: Option<u16> = arg_val("--ws").and_then(|s| s.parse().ok()).or_else(|| {
        std::env::var("RSTUI_PLUGIN_WS")
            .ok()
            .and_then(|s| s.parse().ok())
    });
    let lp = args.iter().any(|a| a == "--lp")
        || std::env::var("RSTUI_PLUGIN_LP").is_ok_and(|v| v != "0" && !v.is_empty());

    // Precedence: shared memory → Unix socket → websocket → stdio.
    if let Some(path) = shm {
        let _ = serve_shm(&path, proto, handler);
    } else if let Some(path) = uds {
        let _ = serve_unix(&path, lp, proto, handler);
    } else if let Some(port) = ws {
        let _ = serve_ws(("127.0.0.1", port), proto, handler);
    } else if lp {
        serve_stdio_lp(proto, handler);
    } else {
        serve(proto, handler);
    }
}
