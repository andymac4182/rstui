// A minimal shared-memory "plugin" for the RTT harness: attach to the
// host's segment (--shm <path>), then echo every message back. Drives the
// addon with the same adaptive poll the TS SDK bridge will use: drain all
// ready messages, then re-arm via setImmediate while active (≈ one
// event-loop tick — the realized Node latency floor) and a 1 ms timer
// when idle (≈0 % CPU). No background thread, no native callbacks.
import { ShmChannel, available } from "./index.mjs";

const argv = process.argv.slice(2);
const i = argv.indexOf("--shm");
const path = i >= 0 ? argv[i + 1] : process.env.RSTUI_PLUGIN_SHM;
if (!available || !path) {
  console.error("shm addon unavailable or no --shm path");
  process.exit(1);
}

const ch = ShmChannel.open(path);
let hotUntil = 0;

function pump() {
  if (ch.isClosed()) {
    process.exit(0);
  }
  let did = false;
  for (;;) {
    const msg = ch.tryRecv();
    if (!msg) break;
    ch.send(msg); // echo
    did = true;
  }
  if (did) hotUntil = Date.now() + 4;
  if (Date.now() < hotUntil) setImmediate(pump);
  else setTimeout(pump, 1);
}
pump();
