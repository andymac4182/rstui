//! `rstui-devtools` — opt-in, in-process performance tracking and
//! Chrome-DevTools-style introspection for apps built on rstui (ADR 0018).
//!
//! This crate is a **dev/debug leaf**: it depends on
//! `rstui-core`/`-widgets`/`-runtime`, never the reverse, and the shipped
//! libraries gain no dependency and keep `unsafe_code = "forbid"`. You add
//! it when you want to *measure your own app*; nothing in rstui's core is
//! affected by its existence.
//!
//! # What it gives you
//!
//! - [`alloc::CountingAllocator`] — a `#[global_allocator]`-installable
//!   shim over the system allocator that counts every allocation,
//!   deallocation, and live/peak byte with atomics. One line in your
//!   binary and every keystroke's heap cost is observable.
//! - [`PerfSession`] — a caller-owned ring buffer of per-frame
//!   [`FrameSample`]s (phase `Duration`s, the RT-01 `produced` flag,
//!   coalesced input count, input→frame latency, per-frame allocation
//!   delta) plus order-statistic [`Aggregate`]s (min/median/p95/p99/max).
//!   It is *model state you own* — the ADR-0012 `ScrollState`/`Input`
//!   seam — not retained widget state.
//!
//! - [`DevToolsAdapter`] — bridges the runtime
//!   [`FrameObserver`](rstui_runtime::FrameObserver) (ADR 0018 §3) to a
//!   caller-owned [`PerfMeter`], pairing each frame's phase timings with
//!   its [`CountingAllocator`](alloc::CountingAllocator) heap delta.
//! - [`DevTools`] — a Chrome-DevTools-style overlay (ADR 0018 §5), a pure
//!   projection of the [`PerfMeter`] (Performance / Memory / Events /
//!   Inspect tabs), built only from existing `rstui-widgets` primitives.
//!
//! # Allocation tracking — one line
//!
//! ```ignore
//! // in your binary's main.rs
//! #[global_allocator]
//! static GLOBAL: rstui_devtools::alloc::CountingAllocator =
//!     rstui_devtools::alloc::CountingAllocator::system();
//! ```
//!
//! Then anywhere: `let snap = rstui_devtools::alloc::snapshot();` and
//! `snap.delta(&earlier)` for the bytes/allocs a span of work cost.
//! Snapshotting never allocates (it only reads atomics), so it is safe to
//! call inside a frame observer.
//!
//! # Why a separate crate (ADR 0018 §2)
//!
//! A `GlobalAlloc` impl is irreducibly `unsafe`; the workspace forbids
//! `unsafe_code` (ADR 0003 §1) and no `#[allow]` lifts a `forbid`. Rather
//! than weaken the guarantee for everyone *using* rstui's core to serve a
//! debug-only need, the one audited allocator shim lives here, in a crate
//! you opt into, held to the same lint bar as the rest of the workspace
//! except that single line.

pub mod alloc;
mod observer;
pub mod overlay;
mod session;

pub use observer::{DevToolsAdapter, PerfMeter};
pub use overlay::DevTools;
pub use session::{Aggregate, FrameSample, PerfSession};
