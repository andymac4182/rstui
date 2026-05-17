//! Cross-process smoke + RTT for the *production* `ShmChannel` (not the
//! ADR 0016 throwaway). Self-execs: parent `create`s the segment and
//! spawns this binary as the child, which `open`s it and echoes; the
//! parent times the round-trip.
//!
//! ```text
//! cargo run --release --example shm_rtt -p rstui-acp-shm
//! ```

use rstui_acp_shm::ShmChannel;
use std::time::Instant;

const WARM: usize = 5_000;
const ITERS: usize = 200_000;

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("child") {
        let path = args.next().expect("path arg");
        let mut c = ShmChannel::open(&path).expect("open");
        while let Ok(Some(m)) = c.recv() {
            if c.send(&m).is_err() {
                break;
            }
        }
        return;
    }

    let path = format!("/tmp/rstui-shm-rtt-{}.bin", std::process::id());
    let mut host = ShmChannel::create(&path).expect("create");
    let exe = std::env::current_exe().expect("exe");
    let mut child = std::process::Command::new(exe)
        .arg("child")
        .arg(&path)
        .spawn()
        .expect("spawn child");

    let payload = vec![0xABu8; 128];
    for _ in 0..WARM {
        host.send(&payload).expect("send");
        host.recv().expect("recv").expect("not eof");
    }

    let mut lat = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let t0 = Instant::now();
        host.send(&payload).expect("send");
        let back = host.recv().expect("recv").expect("not eof");
        lat.push(t0.elapsed().as_nanos() as u64);
        assert_eq!(back.len(), payload.len());
    }
    lat.sort_unstable();
    let pct = |p: usize| -> f64 { lat[(lat.len() * p / 100).min(lat.len() - 1)] as f64 / 1000.0 };
    println!("production ShmChannel — Rust↔Rust RTT (128-byte payload)");
    println!(
        "  min {:.3}  p50 {:.3}  p90 {:.3}  p99 {:.3}  max {:.3}  (µs, {ITERS} iters)",
        lat[0] as f64 / 1000.0,
        pct(50),
        pct(90),
        pct(99),
        *lat.last().expect("non-empty") as f64 / 1000.0,
    );

    drop(host); // closes the segment → child recv returns None → exits
    let _ = child.wait();
}
