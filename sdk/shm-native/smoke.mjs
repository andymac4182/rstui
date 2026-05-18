// 5c correctness smoke: a real `definePlugin` plugin whose transport is
// chosen by the SDK's own bridge() — run with `--shm <path>` it takes
// the shared-memory path (probed native addon). Registers a command and
// answers it with a note, so the Rust host can assert the full
// JSON-RPC-over-shm round-trip through the actual SDK code.
import { definePlugin } from "../ts/index.mjs";

definePlugin({
  onInit(_info, host) {
    host.registerCommand("ping", "shm smoke");
  },
  onCommand(name, _args, host) {
    if (name === "ping") host.note("pong");
  },
});
