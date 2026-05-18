//! Optional Node/Bun shared-memory addon (ADR 0016 / ADR 0019).
//!
//! A thin napi-rs wrapper over [`rstui_acp_shm::ShmChannel`] — the plugin
//! (opener) side. This crate writes **no `unsafe`**, but the `#[napi]`
//! macros expand to `unsafe extern "C"` N-API bindings, so (like
//! `rstui-acp-shm`) it is a sanctioned `unsafe` boundary with its own
//! `[lints]` rather than inheriting the workspace `unsafe_code = forbid`;
//! the audited mmap/atomic/semaphore `unsafe` stays in `rstui-acp-shm`.
//! The API is **synchronous and non-blocking** (`tryRecv` +
//! `isClosed`): the TS SDK drives an adaptive poll on the event loop, so
//! there is no background thread, no `ThreadsafeFunction`, and no `Send`
//! requirement on the (non-`Send`) channel. The realized Node latency is
//! therefore bounded by the JS poll cadence / event loop, not the ~330 ns
//! ring — the caveat ADR 0019 records and measures.

use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use rstui_acp_shm::ShmChannel as Inner;

fn to_err(e: std::io::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// One end of a shared-memory channel (the plugin attaches to a segment
/// the Rust host created). JS class name: `ShmChannel`.
#[napi(js_name = "ShmChannel")]
pub struct ShmChannelJs {
    inner: Inner,
}

#[napi]
impl ShmChannelJs {
    /// Attach to the host-created segment at `path`
    /// (the value of `--shm <path>` / `RSTUI_PLUGIN_SHM`).
    #[napi(factory)]
    pub fn open(path: String) -> napi::Result<Self> {
        Inner::open(&path)
            .map(|inner| Self { inner })
            .map_err(to_err)
    }

    /// Frame and send one message (one JSON-RPC payload).
    #[napi]
    pub fn send(&mut self, data: Buffer) -> napi::Result<()> {
        self.inner.send(&data).map_err(to_err)
    }

    /// Non-blocking: the next whole message if one is ready now, else
    /// `null` (meaning "nothing yet" — **not** EOF; check `isClosed`).
    #[napi]
    pub fn try_recv(&mut self) -> napi::Result<Option<Buffer>> {
        self.inner
            .try_recv()
            .map(|o| o.map(Buffer::from))
            .map_err(to_err)
    }

    /// `true` once the peer closed or the segment was orphaned.
    #[napi]
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}
