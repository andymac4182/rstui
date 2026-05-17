// TypeScript port of rstui-acp-plugin-history (automatic prompt log panel).
import { definePlugin } from "../ts/index.mjs";

function stamp() {
  const s = Math.floor(Date.now() / 1000) % 86400;
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}`;
}
function oneLine(text, max) {
  const flat = text.replace(/\n/g, " ");
  return [...flat].length > max ? `${[...flat].slice(0, max).join("")}…` : flat;
}

const hist = [];
const panelBody = () =>
  hist.length === 0 ? [] : hist.slice().reverse().slice(0, 20);

await definePlugin({
  onInit(_i, host) {
    host.registerCommand(
      "history",
      'Recent prompts ("/history clear" to wipe)',
    );
  },
  onPrompt(text, host) {
    if (text.startsWith("/")) return; // slash commands aren't prompts
    hist.push(`[${stamp()}] ${oneLine(text, 60)}`);
    host.setStatus("history", `${hist.length} prompts`);
    host.panel("Prompt history", panelBody());
  },
  onCommand(name, args, host) {
    if (name !== "history") return;
    if (args.trim() === "clear") {
      hist.length = 0;
      host.setStatus("history", "");
      host.panel("Prompt history", panelBody());
      host.note("prompt history cleared");
    } else if (hist.length === 0) {
      host.note("no prompts yet");
    } else {
      host.note(`${hist.length} prompts recorded (see the sidebar)`);
      host.panel("Prompt history", panelBody());
    }
  },
});
