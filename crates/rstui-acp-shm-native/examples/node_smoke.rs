//! 5c end-to-end smoke: drive a real SDK `definePlugin` plugin over
//! shared memory. The Rust host creates the segment, spawns
//! `node sdk/shm-native/smoke.mjs --shm <path>` (its transport is chosen
//! by the SDK's own `bridge()` → the probed native addon), then asserts
//! the JSON-RPC handshake + a `command/invoke` → `ui/note` round-trip.
//!
//! Build the addon first so the loader resolves it:
//! ```text
//! cargo build -p rstui-acp-shm-native
//! cargo run -p rstui-acp-shm-native --example node_smoke
//! ```

use rstui_acp_shm::ShmChannel;
use std::time::{Duration, Instant};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"type":"init","api_version":"1","client":"smoke","cwd":"."}}"#;
const CMD: &str = r#"{"jsonrpc":"2.0","method":"command/invoke","params":{"type":"command","name":"ping","args":""}}"#;

fn recv_until(ch: &mut ShmChannel, needle: &str, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match ch.recv() {
            Ok(Some(bytes)) => {
                let s = String::from_utf8_lossy(&bytes);
                if s.contains(needle) {
                    return;
                }
            }
            Ok(None) => panic!("plugin closed before `{what}`"),
            Err(e) => panic!("recv error waiting for `{what}`: {e}"),
        }
        assert!(Instant::now() < deadline, "timeout waiting for `{what}`");
    }
}

fn main() {
    let smoke = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../sdk/shm-native/smoke.mjs"
    );
    let path = format!("/tmp/rstui-shm-smoke-{}.bin", std::process::id());
    let mut host = ShmChannel::create(&path).expect("create segment");
    let node = std::env::var("NODE").unwrap_or_else(|_| "node".into());
    let mut child = std::process::Command::new(&node)
        .arg(smoke)
        .arg("--shm")
        .arg(&path)
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("spawn node smoke plugin");

    host.send(INIT.as_bytes()).expect("send initialize");
    recv_until(&mut host, "\"result\"", "initialize ack");
    host.send(CMD.as_bytes()).expect("send command");
    recv_until(&mut host, "ui/note", "ui/note from onCommand");

    println!("SHM-SMOKE OK — definePlugin handshake + command→note over shared memory");
    drop(host); // closes the segment → plugin bridge finishes → node exits
    let _ = child.wait();
    let _ = std::fs::remove_file(&path);
}
