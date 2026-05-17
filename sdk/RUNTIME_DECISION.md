# Decision: how to run TypeScript plugins

Status: **Accepted** · Scope: `rstui-acp-client` plugin runtime · Relates to
[ADR 0007](../docs/adr/0007-plugin-host-and-secure-execution.md) (plugin
host & secure execution).

## Question

Should TypeScript/JS plugins run inside a **`secure-exec` V8 isolate**, or
some other way?

## Context (what we already are)

A plugin — Rust *or* TS — is a **separate OS process** speaking JSON-RPC
2.0 to the client over stdio/websocket. Native Rust plugins run with **no
extra sandbox**; the trust boundary is the OS process plus the typed,
capability-shaped protocol (the host only ever receives `PluginAction`s; a
plugin only ever receives `HostEvent`s). That is exactly ADR 0007's stance:
*separate process, host-mediated; resource/syscall hardening is an operator
concern because the framework forbids `unsafe` and cannot `seccomp` itself.*

So the real question is narrower: **do TS plugins need a V8 isolate that
native plugins don't?** They are not inherently less trusted than the Rust
plugins we already run as plain processes.

## Evidence (hands-on, not theoretical)

`secure-exec` was installed and the isolate path was implemented to its
documented `createInMemoryFileSystem` / `bindings` pattern and run:

| Finding | Detail |
|---|---|
| Pre-1.0, large surface | v0.2.x; ~184 npm deps; 6 packages; a **per-platform native V8 binary** downloaded on install (supply-chain, offline, CI weight). |
| Node ≥22 only | Pulls a whole second runtime + npm tree into an otherwise dependency-disciplined Rust workspace. |
| Real bugs hit & fixed | `cpuTimeLimitMs:0` rejected; host-fs `import()` impossible (had to mount into the in-memory VFS). |
| **Architectural mismatch (blocking)** | `run()` is **bounded execution returning `exports`**. Our plugin is a **long-lived host-driven event loop** (`while (ev = await next())`). After the VFS fix the plugin still never initialised inside the isolate; making it fit needs a different integration (one `run()` per event with kernel-persisted state) — a redesign, not a patch. |

Verified alternative, zero new dependencies: **Node's built-in Permission
Model** (Node 20+, Node 24 here). Proven locally: under
`node --permission --allow-fs-read=…` an attempted `fs.writeFileSync`
fails with `ERR_ACCESS_DENIED` while stdio still works. It gates
`fs-read/fs-write`, `child-process`, `worker`, `addons`, `wasi`. (It does
**not** yet gate **network** — a known Node limitation; covered by an OS
sandbox per ADR 0007.) It fits our long-lived process model with no code
contortions.

## Options considered

1. **secure-exec V8 isolate** — strongest in-process isolation, but native
   binary, pre-1.0 churn, heavy npm tree, and a bounded-execution model
   that fights our event loop (empirically unworkable as the default).
2. **`isolated-vm`** — mature V8 isolates, fits long-lived use better than
   secure-exec, but still a native addon + an extra heavy dep, and (like
   any in-Node isolate) still inside the Node process trust domain.
3. **`vm2`** — unmaintained, known sandbox-escape CVEs → **rejected.**
4. **QuickJS embedded in Rust (`rquickjs`)** — no Node at all, MIT,
   deny-by-default by construction; but ES2020-only (no Node APIs/libs),
   and it breaks the *separate-process* model ADR 0007 deliberately uses.
5. **Uniform separate-process model + Node Permission Model** *(chosen)* —
   TS plugins are processes exactly like Rust plugins; harden the process
   with Node's built-in `--permission` flags. Zero new deps, cross-
   platform, offline/CI-clean, fits the event loop, consistent with
   ADR 0007. Network not gated by Node → OS sandbox where required.

## Decision

- **Do NOT depend on `secure-exec`, and do not make it the supported
  path.** TS plugins run as a normal process speaking the JSON-RPC wire —
  identical to Rust plugins. The SDK works **with or without** the V8 host
  (a built-in stdio bridge), so `node plugin.mjs` / `bun plugin.ts` *is* a
  plugin; no special host or native dependency is required.
- **Recommended hardening (zero deps):** launch the plugin under Node's
  Permission Model — e.g. `node --permission --allow-fs-read=<plugin-dir>
  plugin.mjs` (the host also accepts `--harden` to apply this itself).
  This denies fs-write / child-process / workers / native addons by
  default. For **network** isolation (not covered by Node yet) run the
  plugin under an OS sandbox (`sandbox-exec`, container, firejail) — the
  same operator responsibility ADR 0007 already states.
- **`secure-exec` stays optional + experimental** behind `--sandbox`,
  clearly labelled, for operators who want a full V8 isolate and accept
  its costs and current limitations. Not a workspace dependency.

## Consequences

- TS plugins are first-class with **no native dep, no Node-version lock
  beyond ESM, no npm install** for the common case; `cargo deny`/MSRV are
  untouched (the JS side stays out of the cargo workspace).
- The trust model is uniform and honestly documented: process isolation +
  typed capability protocol + (recommended) Node permissions; network
  hardening is an OS-sandbox/operator concern (ADR 0007).
- We lose in-process V8 isolation by default — accepted, because it is no
  weaker than how native plugins already run, and stronger isolation is
  available opt-in.

## Revisit if

`secure-exec` reaches a stable (≥1.0) release **and** supports a long-
lived host-driven loop (persistent isolate with bidirectional bindings,
not just bounded `run()`), **or** Node's Permission Model gains network
gating (then drop the OS-sandbox caveat).
