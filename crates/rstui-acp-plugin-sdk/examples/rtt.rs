//! Rust↔Rust stdio round-trip micro-bench.
//!
//! `sdk/bench/RESULTS.md` measures a Rust plugin against a *Node* harness,
//! so its latency includes libuv + V8 + Promise/microtask overhead on the
//! measuring side — that is not the production path. The real consumer is
//! the Rust client driving an SDK plugin over pipes. This example removes
//! Node from the picture: it self-execs as the SDK plugin and times the
//! round-trip from a tight Rust host loop, so the number reflects the
//! **transport floor** (two pipe traversals + two process wake-ups +
//! one JSON encode/decode each way), nothing else.
//!
//! Run:
//! ```text
//! cargo run --release --example rtt -p rstui-acp-plugin-sdk
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

const WARM: usize = 2_000;
const ITERS: usize = 50_000;

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"type":"init","api_version":"1","client":"rtt","cwd":"."}}"#;
const CMD: &str = r#"{"jsonrpc":"2.0","method":"command/invoke","params":{"type":"command","name":"x","args":""}}"#;
const SHUTDOWN: &str = r#"{"jsonrpc":"2.0","method":"shutdown","params":{"type":"shutdown"}}"#;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("plugin") => run_plugin(args.next().as_deref() == Some("lp")),
        _ => run_host(),
    }
}

/// The plugin half: the real SDK loop, answering each command with one note.
fn run_plugin(lp: bool) {
    use rstui_acp_plugin_sdk::{HostEvent, PluginAction};
    let handler = |ev: HostEvent, emit: &mut dyn FnMut(PluginAction)| {
        if let HostEvent::Command { .. } = ev {
            emit(PluginAction::Note { text: "x".into() });
        }
    };
    if lp {
        rstui_acp_plugin_sdk::serve_stdio_lp(handler);
    } else {
        rstui_acp_plugin_sdk::serve(handler);
    }
}

/// Frame one message to the child the way the SDK expects, then flush so
/// the read side wakes immediately (one `write()` syscall per message).
fn send(w: &mut ChildStdin, lp: bool, s: &str) {
    if lp {
        let body = s.as_bytes();
        let n = u32::try_from(body.len()).expect("frame fits u32");
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&n.to_be_bytes());
        frame.extend_from_slice(body);
        w.write_all(&frame).expect("write frame");
    } else {
        w.write_all(s.as_bytes()).expect("write line");
        w.write_all(b"\n").expect("write newline");
    }
    w.flush().expect("flush");
}

/// Read exactly one inbound message (strict 1:1 with `send`); the bytes
/// are discarded — we only need the round-trip edge, not the payload.
fn recv(r: &mut BufReader<ChildStdout>, lp: bool, scratch: &mut Vec<u8>) {
    scratch.clear();
    if lp {
        let mut len = [0u8; 4];
        r.read_exact(&mut len).expect("read length prefix");
        let n = u32::from_be_bytes(len) as usize;
        scratch.resize(n, 0);
        r.read_exact(scratch).expect("read body");
    } else {
        let n = r.read_until(b'\n', scratch).expect("read line");
        assert!(n > 0, "plugin closed mid-stream");
    }
}

fn pct(sorted: &[u64], p: usize) -> f64 {
    let i = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[i] as f64 / 1000.0
}

fn run_host() {
    let exe = std::env::current_exe().expect("current exe");
    println!("Rust↔Rust stdio RTT (host loop in Rust, plugin = the SDK)");
    println!("warm-up {WARM}, measured {ITERS}, single in-flight\n");
    println!(
        "{:<10} {:>9} {:>9} {:>9}",
        "framing", "min µs", "p50 µs", "p95 µs"
    );

    for &(label, lp) in &[("stdio", false), ("stdio-lp", true)] {
        let mut child: Child = Command::new(&exe)
            .arg("plugin")
            .arg(if lp { "lp" } else { "nl" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn plugin");
        let mut win = child.stdin.take().expect("child stdin");
        let mut rout = BufReader::new(child.stdout.take().expect("child stdout"));
        let mut scratch = Vec::with_capacity(256);

        // Handshake: one request → one ack response.
        send(&mut win, lp, INIT);
        recv(&mut rout, lp, &mut scratch);

        for _ in 0..WARM {
            send(&mut win, lp, CMD);
            recv(&mut rout, lp, &mut scratch);
        }

        let mut lat = Vec::with_capacity(ITERS);
        for _ in 0..ITERS {
            let t0 = Instant::now();
            send(&mut win, lp, CMD);
            recv(&mut rout, lp, &mut scratch);
            lat.push(t0.elapsed().as_nanos() as u64);
        }
        lat.sort_unstable();
        println!(
            "{label:<10} {:>9.2} {:>9.2} {:>9.2}",
            lat[0] as f64 / 1000.0,
            pct(&lat, 50),
            pct(&lat, 95),
        );

        send(&mut win, lp, SHUTDOWN);
        let _ = child.wait();
    }
}
