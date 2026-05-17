// TypeScript port of rstui-acp-plugin-powerline (behaviour-equivalent to
// the Rust bin). Run: rstui-acp-client --plugin "node sdk/plugins/powerline.plugin.mjs"
import { definePlugin } from "../ts/index.mjs";

const VIBES = [
  "engage ⚡",
  "make it so ✦",
  "warp 9 ➤",
  "aye captain ⌁",
  "scanning… ◎",
  "steady ▰",
];

function clock() {
  const s = Math.floor(Date.now() / 1000) % 86400;
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)} UTC`;
}

function agentLabel(command) {
  const token =
    command
      .split(/\s+/)
      .filter((t) => !t.startsWith("-"))
      .pop() || command;
  const at = token.lastIndexOf("@");
  const noVer = at > 0 ? token.slice(0, at) : token;
  return noVer.split("/").pop() || noVer;
}

const st = { agent: "", cwd: "", prompts: 0, vibe: 0 };

function footer(host) {
  const dir =
    st.cwd.split(/[/\\]/).filter(Boolean).pop() || "~";
  const agent = st.agent ? agentLabel(st.agent) : "no agent";
  host.footer([
    { text: `⛭ ${agent}`, fg: "black", bg: "cyan" },
    { text: `⎇ ${dir}`, fg: "white", bg: "blue" },
    { text: VIBES[st.vibe % VIBES.length], fg: "black", bg: "green" },
    { text: `✉ ${st.prompts}`, fg: "black", bg: "yellow" },
    { text: clock(), fg: "white", bg: "gray" },
  ]);
}

function bump(host) {
  st.vibe = (st.vibe + 1) >>> 0;
  host.setStatus("vibe", VIBES[st.vibe % VIBES.length]);
  footer(host);
}

await definePlugin({
  onInit(info, host) {
    st.cwd = info.cwd;
    host.log("powerline footer online");
    footer(host);
  },
  onSessionStart(agent, host) {
    st.agent = agent;
    footer(host);
  },
  onPrompt(_text, host) {
    st.prompts += 1;
    footer(host);
  },
  onTick: bump,
  onTurnEnded: (_r, host) => bump(host),
});
