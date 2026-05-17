// TypeScript port of rstui-acp-plugin-session (live stopwatch + counters).
import { definePlugin } from "../ts/index.mjs";

let st = { start: Date.now(), prompts: 0, turns: 0 };

function mmss(ms) {
  const s = Math.floor(ms / 1000);
  const p = (n) => String(n).padStart(2, "0");
  return s >= 3600
    ? `${Math.floor(s / 3600)}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`
    : `${p(Math.floor(s / 60))}:${p(s % 60)}`;
}
const elapsed = () => Date.now() - st.start;

function footer(host) {
  host.footer([
    { text: `⏱ ${mmss(elapsed())}`, fg: "black", bg: "magenta" },
    { text: `✦ ${st.turns}t ${st.prompts}p`, fg: "white", bg: "blue" },
  ]);
}
function status(host) {
  host.setStatus(
    "session",
    `${mmss(elapsed())} · ${st.turns} turns · ${st.prompts} prompts`,
  );
}

await definePlugin({
  onInit(_i, host) {
    host.registerCommand("session", "Show this session's stopwatch & counters");
    footer(host);
  },
  onSessionStart(_a, host) {
    st = { start: Date.now(), prompts: 0, turns: 0 };
    footer(host);
    status(host);
  },
  onPrompt(_t, host) {
    st.prompts += 1;
    footer(host);
    status(host);
  },
  onTurnEnded(_r, host) {
    st.turns += 1;
    footer(host);
    status(host);
  },
  onTick: (host) => footer(host),
  async onCommand(name, _args, host) {
    if (name !== "session") return;
    const button = await host.modal(
      "Session",
      [
        `elapsed   ${mmss(elapsed())}`,
        `turns     ${st.turns}`,
        `prompts   ${st.prompts}`,
      ],
      ["Reset", "Close"],
    );
    if (button === "Reset") {
      st = { start: Date.now(), prompts: 0, turns: 0 };
      footer(host);
      status(host);
      host.note("session counters reset");
    }
  },
});
