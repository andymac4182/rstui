// Loader for the optional shared-memory addon (ADR 0019). Resolves the
// platform binary and exposes { ShmChannel, available }. Designed to be
// *probed*: the core TS SDK try/catches this import and falls back to
// uds/stdio when the addon is not built/installed — so the SDK stays
// dependency-free and the fast path is purely additive.
//
// Resolution order:
//   1. RSTUI_SHM_NATIVE — explicit path to a built binary (dev/CI).
//   2. @rstui-acp/plugin-shm-native-<triple> — the per-platform npm
//      package (5d): an optionalDependency, so npm installs only the one
//      matching this host and `require` resolves it with zero config.
//   3. ./shm_native.node — a local convenience copy.
//   4. the workspace cargo target dir — for in-repo `cargo build` use.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", ".."); // crates/* build into the ws target

// napi-style target triple: platform-arch[-libc/abi].
function triples() {
  const a = process.arch; // arm64 | x64 | …
  if (process.platform === "darwin") return [`darwin-${a}`];
  if (process.platform === "win32") return [`win32-${a}-msvc`];
  if (process.platform === "linux") {
    // glibc vs musl: a glibc runtime version means gnu, else musl. Try
    // the likely one first, then the other (cheap, require() just fails).
    let glibc = false;
    try {
      glibc = !!process.report?.getReport?.()?.header?.glibcVersionRuntime;
    } catch {
      /* not available — fall through to trying both */
    }
    return glibc
      ? [`linux-${a}-gnu`, `linux-${a}-musl`]
      : [`linux-${a}-musl`, `linux-${a}-gnu`];
  }
  return [];
}

const dl = process.platform === "darwin" ? "dylib" : "so";
const wsLib = `librstui_acp_shm_native.${dl}`;

// Node loads a native addon by `.node` extension via require; for the raw
// cdylib (.dylib/.so) use process.dlopen into a throwaway module object.
function load(p) {
  if (p.endsWith(".node")) return require(p);
  const m = { exports: {} };
  process.dlopen(m, p);
  return m.exports;
}

const candidates = [
  process.env.RSTUI_SHM_NATIVE,
  ...triples().map((t) => `@rstui-acp/plugin-shm-native-${t}`),
  join(here, "shm_native.node"),
  join(repo, "target", "release", wsLib),
  join(repo, "target", "debug", wsLib),
];

let addon = null;
for (const cand of candidates) {
  if (!cand) continue;
  try {
    addon = load(cand);
    if (addon?.ShmChannel) break;
  } catch {
    /* not present — try next */
  }
}

export const ShmChannel = addon?.ShmChannel ?? null;
export const available = !!ShmChannel;
