//! A deterministic, runnable walkthrough of the rstui plugin security model
//! (ADR 0007): a scripted plugin makes four capability calls; the host
//! mediates every one against the manifest-derived policy and only ever
//! reaches the effect for the *granted* ones.
//!
//! Run it: `cargo run -p rstui-plugin-host --example permissioned_plugin`
//!
//! It uses the in-memory [`FakeProcessRunner`] so it needs no external
//! plugin binary and is fully deterministic — the same conversation a real
//! [`StdProcessRunner`](rstui_plugin_host::std_process::StdProcessRunner)
//! plugin would have over real pipes, with the wire bytes scripted here.
//!
//! ## Wiring a plugin run into an `rstui-runtime` app (the integration seam)
//!
//! `rstui-plugin-host` deliberately depends on nothing — not on
//! `rstui-runtime`, not on any widget — so it cannot and does not reach
//! into the event loop. The decoupled seam is the value it returns:
//! [`PluginRunReport`]. A plugin-aware `App` runs a plugin as an ordinary
//! side effect and folds the report back as a message, exactly like any
//! other `Cmd`:
//!
//! ```ignore
//! // in your App::update, with `host: Arc<PluginHost>` and an owned manifest:
//! Cmd::perform(move || {
//!     let report = host.run_plugin(&manifest, &cwd, Duration::from_secs(5));
//!     Msg::PluginFinished(report)   // a normal app message
//! })
//! ```
//!
//! The host never knows the runtime exists; the runtime never knows the
//! host's internals. `PluginRunReport`/`HostError` is the whole contract.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rstui_plugin_host::capability::{CapabilityRequest, FsMode};
use rstui_plugin_host::clock::FakeClock;
use rstui_plugin_host::effects::RecordingHostEffects;
use rstui_plugin_host::manifest::PluginManifest;
use rstui_plugin_host::message::{CapabilityResponse, encode_request};
use rstui_plugin_host::permission::ManifestPolicy;
use rstui_plugin_host::process::{ExitOutcome, FakePluginProcess, FakeProcessRunner};
use rstui_plugin_host::protocol::{Frame, MessageType, write_frame};
use rstui_plugin_host::{PluginHost, ProcessRunner};

/// Append one plugin→host frame to the scripted stdout byte stream.
fn push(stream: &mut Vec<u8>, message_type: MessageType, correlation: u8, payload: Vec<u8>) {
    let mut id = [0u8; 16];
    id[15] = correlation;
    write_frame(stream, &Frame::new(message_type, id, payload)).expect("script frame");
}

fn main() {
    // 1. The operator-reviewable manifest. It grants exactly two things:
    //    read access under /work/data, and the PATH env var. Everything
    //    else is denied by omission (deny-by-default, ADR 0007 §1/§2).
    let manifest = PluginManifest::parse(concat!(
        "name = \"demo-plugin\"\n",
        "version = \"0.1.0\"\n",
        "api_version = \"1\"\n",
        "entry = \"bin/demo-plugin\"\n",
        "[filesystem]\n",
        "read = \"/work/data\"\n",
        "[env]\n",
        "allow = \"PATH\"\n",
    ))
    .expect("manifest parses");

    // 2. Script what the (fake) plugin will say: the Ready handshake, then
    //    four capability calls — two it is entitled to, two it is not.
    let allowed_read = CapabilityRequest::Filesystem {
        mode: FsMode::Read,
        path: "data/report.csv".into(), // relative -> resolved against cwd
        contents: Vec::new(),           // a read carries no payload
    };
    let escaping_read = CapabilityRequest::Filesystem {
        mode: FsMode::Read,
        path: "data/../../etc/passwd".into(), // canonicalised, then denied
        contents: Vec::new(),
    };
    let allowed_env = CapabilityRequest::Env { key: "PATH".into() };
    let denied_env = CapabilityRequest::Env {
        key: "AWS_SECRET_ACCESS_KEY".into(),
    };

    let mut stdout = Vec::new();
    push(&mut stdout, MessageType::Ready, 0, Vec::new());
    push(
        &mut stdout,
        MessageType::CapabilityCall,
        1,
        encode_request(&allowed_read),
    );
    push(
        &mut stdout,
        MessageType::CapabilityCall,
        2,
        encode_request(&escaping_read),
    );
    push(
        &mut stdout,
        MessageType::CapabilityCall,
        3,
        encode_request(&allowed_env),
    );
    push(
        &mut stdout,
        MessageType::CapabilityCall,
        4,
        encode_request(&denied_env),
    );

    // 3. Assemble the host from the four injected seams. RecordingHostEffects
    //    lets us prove the denied calls never reached the effector.
    let runner: Arc<dyn ProcessRunner> = Arc::new(FakeProcessRunner::new(FakePluginProcess::new(
        stdout,
        Vec::new(),
        ExitOutcome {
            code: Some(0),
            success: true,
        },
    )));
    let policy = Arc::new(ManifestPolicy::from_manifest(&manifest));
    let effects = Arc::new(RecordingHostEffects::with_ok(b"<effect result>".to_vec()));
    let clock = Arc::new(FakeClock::new());

    let host = PluginHost::new(
        runner,
        policy,
        effects.clone(),
        clock,
        "1", // host api_version; must match the manifest's
    );

    // 4. Run it. The plugin's cwd is /work, so `data/report.csv` resolves to
    //    /work/data/report.csv (inside the grant) and `data/../../etc/passwd`
    //    canonicalises to /etc/passwd (outside it).
    let report = host
        .run_plugin(&manifest, Path::new("/work"), Duration::from_secs(5))
        .expect("the run completes");

    println!("plugin `{}` exited {:?}\n", report.plugin, report.exit);
    println!("mediated {} capability call(s):", report.mediated.len());
    for (n, record) in report.mediated.iter().enumerate() {
        let verdict = match &record.response {
            CapabilityResponse::Ok { .. } => "ALLOWED  → effect ran".to_string(),
            CapabilityResponse::Denied { reason } => format!("DENIED   ({reason})"),
            CapabilityResponse::Failed { error } => format!("FAILED   ({error})"),
        };
        println!("  {}. {:?}\n     {}", n + 1, record.request, verdict);
    }

    // 5. The crux of the model: only the two granted calls ever reached the
    //    host effect. The two denied ones were stopped at the policy.
    let reached = effects.calls();
    println!(
        "\nhost effect was invoked {} time(s) — only for the granted calls:",
        reached.len()
    );
    for request in &reached {
        println!("  {request:?}");
    }
    assert_eq!(
        reached.len(),
        2,
        "exactly the two granted calls reach the effect"
    );
    println!("\ninvariant holds: a denied capability never reaches HostEffects.");
}
