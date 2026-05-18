// Loader for the optional shared-memory addon (ADR 0019). Resolves the
// platform binary and exposes { ShmChannel, available }. Designed to be
// *probed*: the core TS SDK try/catches this import and falls back to
// uds/stdio when the addon is not built/installed — so the SDK stays
// dependency-free and the fast path is purely additive.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
// crates/rstui-acp-shm-native builds into the *workspace* target dir.
const repo = join(here, "..", "..");
const dl = process.platform === "darwin" ? "dylib" : "so";
const lib = `librstui_acp_shm_native.${dl}`;

// Node loads a native addon by `.node` extension via require; for the raw
// cdylib (.dylib/.so) use process.dlopen into a throwaway module object.
function load(p) {
  if (p.endsWith(".node")) return require(p);
  const m = { exports: {} };
  process.dlopen(m, p);
  return m.exports;
}

let addon = null;
for (const cand of [
  process.env.RSTUI_SHM_NATIVE, // explicit override (phase 5d packaging)
  join(here, "shm_native.node"), // local convenience copy
  join(repo, "target", "release", lib),
  join(repo, "target", "debug", lib),
]) {
  if (!cand) continue;
  try {
    addon = load(cand);
    break;
  } catch {
    /* try next candidate */
  }
}

export const ShmChannel = addon?.ShmChannel ?? null;
export const available = !!ShmChannel;
