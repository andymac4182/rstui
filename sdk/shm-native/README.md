# @rstui-acp/plugin-shm-native

The **optional** shared-memory transport addon for `@rstui-acp/plugin-sdk`
(ADR 0019). It lets a Node/Bun plugin speak the same shared-memory
transport a Rust plugin uses (ADR 0016) instead of stdio/uds/ws.

## Honest expectations — read this first

This is offered for **transport parity / operator choice, NOT speed.**
Measured Node-over-shm RTT is **≈ Node-stdio** (p50 ~15 µs): the Node
event loop, not the IPC, is the floor — a message still has to cross into
V8 via an event-loop-scheduled callback. The Rust sub-µs win does **not**
transfer to Node. If you need a flat sub-µs tail, write the plugin in
Rust. See [ADR 0019](../../docs/adr/0019-node-shared-memory-via-napi.md)
for the measurement and the reasoning.

## How it stays dependency-free

The core SDK never depends on this. `bridge()` *probes* the addon
(try/catch dynamic import); if it is absent, a TS plugin launched with
`--shm` logs one line and falls back to uds/stdio — no error, no latency
loss (shm ≈ stdio for Node anyway). The prebuilt binaries are
`optionalDependencies`: npm installs only the one matching the host
(`os`/`cpu`/`libc` gated), and none installing is non-fatal.

## Use

```sh
npm i @rstui-acp/plugin-shm-native   # optional; only if you want it
```

Then launch the plugin with `--shm <path>` (or `RSTUI_PLUGIN_SHM=<path>`);
the host (`rstui-acp-client`, a plugin command containing `--shm`) creates
the segment. Nothing else changes — same `definePlugin`, same wire.

## Build locally (no npm)

```sh
cargo build -p rstui-acp-shm-native            # → workspace target dir
# index.mjs auto-resolves it from target/{release,debug}; or:
export RSTUI_SHM_NATIVE=/abs/path/to/built.{dylib,so}
node crates/rstui-acp-shm-native/examples/...  # see the crate's examples
```

Smoke + RTT (Rust host ↔ Node plugin over shm):

```sh
cargo run -p rstui-acp-shm-native --example node_smoke   # correctness
cargo run --release -p rstui-acp-shm-native --example node_rtt  # latency
```

## Packaging (CI)

`.github/workflows/shm-native.yml` cross-builds `<triple>.node` per
platform; `scripts/prepare-packages.mjs` assembles the per-platform npm
sub-packages from the artifacts. Publishing is tag-gated
(`shm-native-v*`) and needs an `NPM_TOKEN` repo secret. The Rust crate
itself lives in `crates/rstui-acp-shm-native/` and is fully gated
(`cargo xtask ci` / deny / machete / MSRV) like every other crate.
