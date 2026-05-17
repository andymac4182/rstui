//! Shared-memory SPSC transport for rstui-acp plugins ([ADR 0016]).
//!
//! Two processes `mmap` one `MAP_SHARED` tmpfile holding two single-
//! producer/single-consumer byte rings (one per direction) and a small
//! control header. A message is `[u32 little-endian length][bytes]`.
//!
//! Latency comes from **scoped adaptive spin**: a reader hot-spins a
//! bounded *stay-hot* window (covering an active request/response
//! exchange — flat sub-µs, see ADR 0016 evidence), then **parks** on a
//! POSIX named semaphore (≈0 % CPU while idle). A coarse watchdog thread
//! makes a hard peer-crash or orphaning recoverable without `prctl`,
//! `kqueue`, or `sem_timedwait`, so the exact same code runs on Linux
//! (CI) and macOS (dev).
//!
//! This is the **single sanctioned `unsafe` crate** (see `Cargo.toml`):
//! every `unsafe` block has a `SAFETY:` note; no other workspace crate
//! gains `unsafe`.
//!
//! [ADR 0016]: https://github.com/andymac4182/rstui/blob/main/docs/adr/0016-shared-memory-plugin-transport.md

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Control header size (one page): plenty for the magic/closed/ready
/// words plus each ring's head/tail/park on its own cache line.
const HDR: usize = 4096;
/// Per-direction ring capacity (1 MiB, power of two). Plugin control
/// messages are tens of bytes; this absorbs large `ui/panel` bursts
/// without ever hitting back-pressure in practice.
const CAP: usize = 1 << 20;
const MASK: u64 = (CAP as u64) - 1;
/// Total mapping size: header + both ring data regions.
const SIZE: usize = HDR + 2 * CAP;

// Header field offsets (8-byte aligned; rings spaced one cache line
// apart so the two directions never false-share).
const O_MAGIC: usize = 0;
const O_CLOSED: usize = 8;
const O_READY: usize = 16;
const O_A_HEAD: usize = 64;
const O_A_TAIL: usize = 128;
const O_A_PARK: usize = 192;
const O_B_HEAD: usize = 256;
const O_B_TAIL: usize = 320;
const O_B_PARK: usize = 384;
const O_A_DATA: usize = HDR;
const O_B_DATA: usize = HDR + CAP;

const MAGIC: u64 = 0x7273_7475_5f73_686d; // "rstu_shm"

/// How long a reader stays hot-spinning before parking. Any exchange
/// with gaps shorter than this never parks → flat sub-µs (ADR 0016).
const STAY_HOT: Duration = Duration::from_micros(200);
/// Watchdog tick. `closed` is checked every tick (so channel teardown —
/// `Drop` joins the watchdog — completes within one tick), while the
/// coarser orphan check runs every [`ORPHAN_TICKS`] ticks. Negligible
/// CPU; only an error/teardown path.
const WATCHDOG: Duration = Duration::from_millis(10);
/// Orphan (`getppid` changed) is checked every Nth [`WATCHDOG`] tick
/// (~250 ms) — coarse is fine for a crash path.
const ORPHAN_TICKS: u32 = 25;

/// One ring's control offsets + data window.
#[derive(Clone, Copy)]
struct Ring {
    head: usize,
    tail: usize,
    park: usize,
    data: usize,
}

const RING_A: Ring = Ring {
    head: O_A_HEAD,
    tail: O_A_TAIL,
    park: O_A_PARK,
    data: O_A_DATA,
};
const RING_B: Ring = Ring {
    head: O_B_HEAD,
    tail: O_B_TAIL,
    park: O_B_PARK,
    data: O_B_DATA,
};

/// FNV-1a of the path → short, collision-resilient POSIX semaphore names
/// (macOS caps names at ~31 chars, so keep them tiny).
fn sem_names(path: &str) -> (CString, CString) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let h = (h as u32) ^ ((h >> 32) as u32);
    (
        CString::new(format!("/r{h:08x}a")).expect("nul-free"),
        CString::new(format!("/r{h:08x}b")).expect("nul-free"),
    )
}

fn sem_open(name: &CString) -> io::Result<*mut libc::sem_t> {
    // SAFETY: `name` is a valid NUL-terminated C string for the call's
    // duration; O_CREAT is idempotent (both peers open the same named
    // semaphore), initial value 0.
    let s = unsafe {
        libc::sem_open(
            name.as_ptr(),
            libc::O_CREAT,
            0o600 as libc::c_uint,
            0 as libc::c_uint,
        )
    };
    if s == libc::SEM_FAILED {
        return Err(io::Error::last_os_error());
    }
    Ok(s)
}

/// One end of a bidirectional shared-memory message channel.
///
/// [`ShmChannel::create`] makes the segment (the host); [`ShmChannel::open`]
/// attaches to it (the plugin). Both then use [`send`](Self::send) /
/// [`recv`](Self::recv). The wire payload is opaque bytes — the SDK layers
/// JSON-RPC framing on top, exactly as for the pipe/socket transports.
pub struct ShmChannel {
    base: *mut u8,
    creator: bool,
    path: String,
    name_a: CString,
    name_b: CString,
    /// Semaphore the *peer reading our TX ring* parks on (we post it).
    sem_tx: *mut libc::sem_t,
    /// Semaphore we park on, waiting on our RX ring (peer posts it).
    sem_rx: *mut libc::sem_t,
    tx: Ring,
    rx: Ring,
    _file: File,
    watchdog: Option<JoinHandle<()>>,
}

// The raw pointers address process-shared memory used single-threaded by
// the owning end (serve loop), with the watchdog touching only the
// `closed` word via a separately-passed address. Not auto-Send/Sync.
impl ShmChannel {
    fn at(&self, off: usize) -> &AtomicU64 {
        // SAFETY: `off` is a fixed, 8-byte-aligned header offset < HDR <
        // SIZE; `base` is a page-aligned live mapping of length SIZE for
        // `self`'s lifetime (munmap happens in Drop after the watchdog
        // joins). `AtomicU64` over `MAP_SHARED` memory is valid for
        // cross-process synchronisation on a single host.
        unsafe { &*(self.base.add(off).cast::<AtomicU64>()) }
    }

    fn map(path: &str, creator: bool) -> io::Result<(File, *mut u8)> {
        let file = if creator {
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;
            f.set_len(SIZE as u64)?;
            f
        } else {
            // The creator may not have sized the file yet — wait briefly.
            let start = Instant::now();
            loop {
                if let Ok(f) = OpenOptions::new().read(true).write(true).open(path) {
                    if f.metadata().map(|m| m.len()).unwrap_or(0) == SIZE as u64 {
                        break f;
                    }
                }
                if start.elapsed() > Duration::from_secs(5) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "shm segment did not appear",
                    ));
                }
                std::thread::sleep(Duration::from_micros(200));
            }
        };
        // SAFETY: fd is valid and the file is exactly SIZE bytes;
        // PROT_READ|WRITE + MAP_SHARED is the documented mmap contract;
        // null hint lets the kernel choose the address.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok((file, p.cast::<u8>()))
    }

    fn finish(base: *mut u8, creator: bool, path: &str, file: File) -> io::Result<Self> {
        let (name_a, name_b) = sem_names(path);
        let sem_a = sem_open(&name_a)?;
        let sem_b = sem_open(&name_b)?;
        // Creator writes A / reads B; opener writes B / reads A. The
        // semaphore of a ring is parked-on by that ring's reader and
        // posted by its writer.
        let (tx, rx, sem_tx, sem_rx) = if creator {
            (RING_A, RING_B, sem_a, sem_b)
        } else {
            (RING_B, RING_A, sem_b, sem_a)
        };
        let mut ch = Self {
            base,
            creator,
            path: path.to_owned(),
            name_a,
            name_b,
            sem_tx,
            sem_rx,
            tx,
            rx,
            _file: file,
            watchdog: None,
        };
        if creator {
            ch.at(O_CLOSED).store(0, Ordering::Release);
            ch.at(O_A_HEAD).store(0, Ordering::Release);
            ch.at(O_A_TAIL).store(0, Ordering::Release);
            ch.at(O_A_PARK).store(0, Ordering::Release);
            ch.at(O_B_HEAD).store(0, Ordering::Release);
            ch.at(O_B_TAIL).store(0, Ordering::Release);
            ch.at(O_B_PARK).store(0, Ordering::Release);
            ch.at(O_MAGIC).store(MAGIC, Ordering::Release);
            ch.at(O_READY).store(1, Ordering::Release);
        } else {
            let start = Instant::now();
            while ch.at(O_READY).load(Ordering::Acquire) != 1 {
                if start.elapsed() > Duration::from_secs(5) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "shm segment not initialised",
                    ));
                }
                std::hint::spin_loop();
            }
            if ch.at(O_MAGIC).load(Ordering::Acquire) != MAGIC {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "bad shm magic"));
            }
        }
        ch.spawn_watchdog();
        Ok(ch)
    }

    /// Create the segment (host side). Truncates/overwrites `path`.
    ///
    /// # Errors
    /// File, `mmap`, or semaphore creation failure.
    pub fn create(path: &str) -> io::Result<Self> {
        let (file, base) = Self::map(path, true)?;
        Self::finish(base, true, path, file)
    }

    /// Attach to a segment created by the peer (plugin side). Waits
    /// briefly for the creator to size and initialise it.
    ///
    /// # Errors
    /// Timeout, `mmap`, bad magic, or semaphore failure.
    pub fn open(path: &str) -> io::Result<Self> {
        let (file, base) = Self::map(path, false)?;
        Self::finish(base, false, path, file)
    }

    fn spawn_watchdog(&mut self) {
        let closed_addr = self.at(O_CLOSED) as *const AtomicU64 as usize;
        let parent = if self.creator {
            0
        } else {
            // SAFETY: getppid is always safe.
            unsafe { libc::getppid() }
        };
        let (na, nb) = (self.name_a.clone(), self.name_b.clone());
        self.watchdog = Some(std::thread::spawn(move || {
            let mut tick: u32 = 0;
            loop {
                std::thread::sleep(WATCHDOG);
                // SAFETY: `closed_addr` is the address of the `closed`
                // AtomicU64 in the live mapping; Drop joins this thread
                // *before* munmap, so the address stays valid here.
                let closed = unsafe { &*(closed_addr as *const AtomicU64) };
                if closed.load(Ordering::Acquire) != 0 {
                    break;
                }
                // Plugin orphaned (host died → reparented): tear down.
                // Coarse — only every ORPHAN_TICKS ticks.
                tick = tick.wrapping_add(1);
                if parent != 0 && tick % ORPHAN_TICKS == 0 {
                    // SAFETY: getppid is always safe.
                    let now = unsafe { libc::getppid() };
                    if now != parent {
                        closed.store(1, Ordering::Release);
                        break;
                    }
                }
            }
            // Unblock a peer/self parked in sem_wait so recv() can
            // observe `closed`. Reopen by name (idempotent).
            for n in [&na, &nb] {
                if let Ok(s) = sem_open(n) {
                    // SAFETY: `s` is a valid open semaphore handle.
                    unsafe {
                        libc::sem_post(s);
                        libc::sem_close(s);
                    }
                }
            }
        }));
    }

    fn closed(&self) -> bool {
        self.at(O_CLOSED).load(Ordering::Acquire) != 0
    }

    /// Copy `src` into ring `data` at running offset `head`, wrapping.
    fn ring_put(&self, data_off: usize, head: u64, src: &[u8]) {
        let pos = (head & MASK) as usize;
        let first = src.len().min(CAP - pos);
        // SAFETY: `pos < CAP`; `first ≤ CAP-pos` and `src.len()-first ≤
        // pos`, so both copies stay within `[data_off, data_off+CAP)` ⊂
        // mapping. Caller guarantees `free ≥ src.len()` (SPSC: we are the
        // sole writer) so this never overruns unread bytes.
        unsafe {
            let d = self.base.add(data_off);
            std::ptr::copy_nonoverlapping(src.as_ptr(), d.add(pos), first);
            if first < src.len() {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(first), d, src.len() - first);
            }
        }
    }

    /// Copy `len` bytes out of ring `data` at running offset `tail`.
    fn ring_take(&self, data_off: usize, tail: u64, len: usize) -> Vec<u8> {
        let pos = (tail & MASK) as usize;
        let first = len.min(CAP - pos);
        let mut out = vec![0u8; len];
        // SAFETY: same bounds reasoning as `ring_put`; we are the sole
        // reader and `len` bytes are known-published (head − tail ≥ len),
        // so these source bytes are valid and stable.
        unsafe {
            let d = self.base.add(data_off);
            std::ptr::copy_nonoverlapping(d.add(pos), out.as_mut_ptr(), first);
            if first < len {
                std::ptr::copy_nonoverlapping(d, out.as_mut_ptr().add(first), len - first);
            }
        }
        out
    }

    /// Send one message (`[u32 LE len][bytes]`) on the TX ring.
    ///
    /// Blocks (scoped spin → brief sleep) only if the ring is full —
    /// effectively never for plugin-sized messages.
    ///
    /// # Errors
    /// Message larger than the ring, or the peer has closed.
    pub fn send(&mut self, msg: &[u8]) -> io::Result<()> {
        let need = 4 + msg.len();
        if need > CAP {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "message exceeds shm ring capacity",
            ));
        }
        let head = self.at(self.tx.head).load(Ordering::Relaxed);
        let spin = Instant::now();
        loop {
            let tail = self.at(self.tx.tail).load(Ordering::Acquire);
            if (CAP as u64) - (head - tail) >= need as u64 {
                break;
            }
            if self.closed() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"));
            }
            if spin.elapsed() < STAY_HOT {
                std::hint::spin_loop();
            } else {
                std::thread::sleep(Duration::from_micros(50));
            }
        }
        let len = u32::try_from(msg.len()).expect("checked ≤ CAP");
        self.ring_put(self.tx.data, head, &len.to_le_bytes());
        self.ring_put(self.tx.data, head + 4, msg);
        // Publish the bytes, then wake the reader if it parked.
        self.at(self.tx.head)
            .store(head + need as u64, Ordering::Release);
        if self.at(self.tx.park).load(Ordering::Acquire) == 1 {
            // SAFETY: `sem_tx` is a valid open semaphore handle.
            unsafe {
                libc::sem_post(self.sem_tx);
            }
        }
        Ok(())
    }

    /// Extract one complete frame from the RX ring if a whole message is
    /// already buffered; pure, non-blocking (no spin, no park). The sole
    /// reader, so caching `tail` between calls is unnecessary.
    fn poll_frame(&self) -> Option<Vec<u8>> {
        let tail = self.at(self.rx.tail).load(Ordering::Relaxed);
        let head = self.at(self.rx.head).load(Ordering::Acquire);
        if head.wrapping_sub(tail) < 4 {
            return None;
        }
        let lb = self.ring_take(self.rx.data, tail, 4);
        let mlen = u64::from(u32::from_le_bytes([lb[0], lb[1], lb[2], lb[3]]));
        if head.wrapping_sub(tail) < 4 + mlen {
            return None; // length arrived, body not yet
        }
        let body = self.ring_take(self.rx.data, tail + 4, mlen as usize);
        self.at(self.rx.tail)
            .store(tail + 4 + mlen, Ordering::Release);
        Some(body)
    }

    /// `true` once either peer has closed (clean `Drop`) or the watchdog
    /// detected an orphan/crash. Pair with [`try_recv`](Self::try_recv) in
    /// a host poll loop to distinguish "nothing yet" from end-of-stream.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed()
    }

    /// Non-blocking receive: `Ok(Some(bytes))` if a whole message is
    /// ready *now*, else `Ok(None)` (which here means "not ready", **not**
    /// EOF — check [`is_closed`](Self::is_closed)). For an asymmetric host
    /// driver that also pushes events and must not park.
    ///
    /// # Errors
    /// Never returns `Err`; the signature mirrors [`recv`](Self::recv).
    pub fn try_recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        Ok(self.poll_frame())
    }

    /// Receive one message, or `Ok(None)` at clean end-of-stream
    /// (peer closed / orphaned).
    ///
    /// Scoped adaptive spin: hot-spins the stay-hot window (flat sub-µs
    /// for an active exchange), then parks on the RX semaphore (≈0 % CPU
    /// when idle).
    ///
    /// # Errors
    /// Never returns `Err` for normal close — that is `Ok(None)`.
    pub fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(body) = self.poll_frame() {
                return Ok(Some(body));
            }
            if self.closed() {
                return Ok(None);
            }
            let tail = self.at(self.rx.tail).load(Ordering::Relaxed);
            // Scoped hot-spin window.
            let spin = Instant::now();
            let mut hot = true;
            while spin.elapsed() < STAY_HOT {
                if self
                    .at(self.rx.head)
                    .load(Ordering::Acquire)
                    .wrapping_sub(tail)
                    >= 4
                {
                    hot = false;
                    break;
                }
                std::hint::spin_loop();
            }
            if !hot {
                continue;
            }
            // Park: arm flag, re-check (lost-wakeup guard), then block.
            self.at(self.rx.park).store(1, Ordering::Release);
            if self
                .at(self.rx.head)
                .load(Ordering::Acquire)
                .wrapping_sub(tail)
                >= 4
                || self.closed()
            {
                self.at(self.rx.park).store(0, Ordering::Release);
                continue;
            }
            // SAFETY: `sem_rx` is a valid open semaphore handle; a peer
            // `sem_post` (on send) or the watchdog (on close/orphan)
            // wakes us. EINTR just re-loops.
            unsafe {
                libc::sem_wait(self.sem_rx);
            }
            self.at(self.rx.park).store(0, Ordering::Release);
            // SAFETY: drain any coalesced posts so a stale token cannot
            // mask a future empty-wait.
            unsafe { while libc::sem_trywait(self.sem_rx) == 0 {} }
        }
    }
}

impl Drop for ShmChannel {
    fn drop(&mut self) {
        // Signal close, wake a peer parked in sem_wait, stop+join the
        // watchdog *before* unmapping (it reads the `closed` address).
        self.at(O_CLOSED).store(1, Ordering::Release);
        // SAFETY: both semaphore handles are valid and open.
        unsafe {
            libc::sem_post(self.sem_tx);
            libc::sem_post(self.sem_rx);
        }
        if let Some(h) = self.watchdog.take() {
            let _ = h.join();
        }
        // SAFETY: `base` is the live mapping of length SIZE returned by
        // `mmap`; no `&AtomicU64` derived from it outlives this call
        // (watchdog joined; `self` is being dropped). Handles are closed
        // once; the creator unlinks the named objects last.
        unsafe {
            libc::munmap(self.base.cast::<libc::c_void>(), SIZE);
            libc::sem_close(self.sem_tx);
            libc::sem_close(self.sem_rx);
            if self.creator {
                libc::sem_unlink(self.name_a.as_ptr());
                libc::sem_unlink(self.name_b.as_ptr());
            }
        }
        if self.creator {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two threads sharing one tmpfile = real cross-mapping shared
    /// memory; exercises ring framing, wrap, and the scoped-spin/park
    /// wake without needing a child process (so it runs under the
    /// `test` gate on Linux CI and macOS alike).
    #[test]
    fn round_trips_many_messages_across_two_mappings() {
        let path = format!(
            "/tmp/rstui-shm-test-{}-{:?}.bin",
            std::process::id(),
            std::thread::current().id()
        );
        let p2 = path.clone();
        let host = ShmChannel::create(&path).expect("create");
        let plug = std::thread::spawn(move || {
            let mut c = ShmChannel::open(&p2).expect("open");
            for _ in 0..5_000 {
                let m = c.recv().expect("recv").expect("not eof");
                c.send(&m).expect("echo"); // echo back
            }
            // Drain the final close.
            while let Ok(Some(_)) = c.recv() {}
        });
        let mut host = host;
        for i in 0..5_000u32 {
            let msg = format!("ping-{i}-{}", "x".repeat((i % 200) as usize));
            host.send(msg.as_bytes()).expect("send");
            let back = host.recv().expect("recv").expect("not eof");
            assert_eq!(back, msg.as_bytes());
        }
        drop(host); // sets closed → plugin recv returns None → thread ends
        plug.join().expect("plugin thread");
    }

    #[test]
    fn rejects_oversized_message() {
        let path = format!("/tmp/rstui-shm-big-{}.bin", std::process::id());
        let mut host = ShmChannel::create(&path).expect("create");
        let too_big = vec![0u8; CAP + 1];
        assert!(host.send(&too_big).is_err());
    }
}
