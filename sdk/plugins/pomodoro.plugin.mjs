// TypeScript port of rstui-acp-plugin-pomodoro (focus countdown timer).
import { definePlugin } from "../ts/index.mjs";

let timer = null; // { endMs, minutes } | null

function mmss(ms) {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 60))}:${p(s % 60)}`;
}
function clear(host) {
  host.footer([]);
  host.setStatus("pomodoro", "");
}

await definePlugin({
  onInit(_i, host) {
    host.registerCommand("pomodoro", "Focus timer: /pomodoro [minutes] | stop");
  },
  onCommand(name, args, host) {
    if (name !== "pomodoro") return;
    const a = args.trim();
    if (a === "stop" || a === "cancel") {
      timer = null;
      clear(host);
      host.note("pomodoro cancelled");
    } else {
      const m = Number.parseInt(a, 10);
      const minutes = Number.isInteger(m) && m > 0 ? m : 25;
      timer = { endMs: Date.now() + minutes * 60000, minutes };
      host.note(`🍅 pomodoro started — ${minutes} min, stay focused`);
    }
  },
  onTick(host) {
    if (!timer) return;
    const rem = timer.endMs - Date.now();
    if (rem > 0) {
      const low = rem < 60000;
      host.footer([
        { text: `🍅 ${mmss(rem)}`, fg: "black", bg: low ? "red" : "green" },
      ]);
      host.setStatus("pomodoro", `${mmss(rem)} left`);
    } else {
      const m = timer.minutes;
      timer = null;
      clear(host);
      host.note(`🍅 pomodoro done after ${m} min — take a break!`);
    }
  },
  onShutdown(host) {
    if (timer) clear(host);
  },
});
