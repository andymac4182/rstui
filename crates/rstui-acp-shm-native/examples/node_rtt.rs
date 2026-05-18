//! Real Node-over-shared-memory RTT (ADR 0019 evidence). The Rust host
//! creates the segment, spawns `node echo.mjs --shm <path>` (the Node
//! plugin uses the napi addon + adaptive poll), and times the round-trip.
//! This measures whether the realized Node latency is bounded by the
//! N-API / event-loop crossing rather than the ~330 ns ring.
//!
//! Build the addon first, then run:
//! ```text
//! (cd sdk/shm-native && cargo build --release \
//!   && cp target/release/librstui_acp_shm_native.dylib shm_native.node \
//!   && cargo run --release --example node_rtt)
//! ```

use rstui_acp_shm::ShmChannel;
use std::time::Instant;

const WARM: usize = 2_000;
const ITERS: usize = 30_000;

fn main() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let echo = format!("{dir}/echo.mjs");
    let path = format!("/tmp/rstui-node-shm-{}.bin", std::process::id());
    let mut host = ShmChannel::create(&path).expect("create");
    let node = std::env::var("NODE").unwrap_or_else(|_| "node".into());
    let mut child = std::process::Command::new(&node)
        .arg(&echo)
        .arg("--shm")
        .arg(&path)
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("spawn node");

    let payload = vec![0xABu8; 128];
    // Let the Node process boot + open the segment.
    for _ in 0..WARM {
        host.send(&payload).expect("send");
        host.recv().expect("recv").expect("echo");
    }

    let mut lat = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let t0 = Instant::now();
        host.send(&payload).expect("send");
        let back = host.recv().expect("recv").expect("echo");
        lat.push(t0.elapsed().as_nanos() as u64);
        assert_eq!(back.len(), payload.len());
    }
    lat.sort_unstable();
    let pct = |p: usize| -> f64 { lat[(lat.len() * p / 100).min(lat.len() - 1)] as f64 / 1000.0 };
    println!("Node-over-shm RTT (napi addon + adaptive poll), {ITERS} iters");
    println!(
        "  min {:.2}  p50 {:.2}  p90 {:.2}  p99 {:.2}  max {:.2}  (µs)",
        lat[0] as f64 / 1000.0,
        pct(50),
        pct(90),
        pct(99),
        *lat.last().expect("non-empty") as f64 / 1000.0,
    );
    println!("(compare: Node-stdio ≈ 10–22 µs p50; Rust-shm ≈ 0.3 µs p50)");

    drop(host);
    let _ = child.wait();
    let _ = std::fs::remove_file(&path);
}
