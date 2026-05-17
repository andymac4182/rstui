// TypeScript port of rstui-acp-plugin-git (branch/dirty footer + /git panel).
// Shells out to `git` (node:child_process) just like the Rust bin; degrades
// cleanly outside a repo / when git is absent. NOTE: child_process is denied
// under the Node Permission Model — run this plugin without --harden.
import { definePlugin } from "../ts/index.mjs";
import { spawnSync } from "node:child_process";

let cwd = process.cwd();

function git(args) {
  const r = spawnSync("git", ["-C", cwd, ...args], { encoding: "utf8" });
  if (r.status !== 0 || r.error) return null;
  return r.stdout.replace(/\s+$/, "");
}
function snapshot() {
  const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch === null) return null;
  const porcelain = git(["status", "--porcelain"]) ?? "";
  const changes = porcelain.split("\n").filter((l) => l.length > 0);
  return { branch, changes };
}
function emitState(host) {
  const s = snapshot();
  if (s) {
    const n = s.changes.length;
    host.footer([
      {
        text: n === 0 ? `⎇ ${s.branch}` : `⎇ ${s.branch} ±${n}`,
        fg: "black",
        bg: n === 0 ? "green" : "yellow",
      },
    ]);
    host.setStatus("git", `${s.branch} (${n} changed)`);
  } else {
    host.footer([{ text: "⎇ —", fg: "white", bg: "gray" }]);
    host.setStatus("git", "not a git repo");
  }
}

await definePlugin({
  onInit(info, host) {
    cwd = info.cwd || cwd;
    host.registerCommand("git", "Show git branch & changed files");
    emitState(host);
  },
  onSessionStart: (_a, host) => emitState(host),
  onTurnEnded: (_r, host) => emitState(host),
  onTick: (host) => emitState(host),
  onCommand(name, _args, host) {
    if (name !== "git") return;
    const s = snapshot();
    let body;
    if (!s) body = ["not a git repository"];
    else if (s.changes.length === 0)
      body = [`on ${s.branch}`, "working tree clean"];
    else
      body = [
        `on ${s.branch} — ${s.changes.length} changed:`,
        ...s.changes.slice(0, 40),
      ];
    host.panel("Git", body);
  },
});
