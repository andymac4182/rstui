// TypeScript port of rstui-acp-plugin-btw (private side-notes).
import { definePlugin } from "../ts/index.mjs";

function stamp() {
  const s = Math.floor(Date.now() / 1000) % 86400;
  const p = (n) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}`;
}

const notes = [];

await definePlugin({
  onInit(_i, host) {
    host.registerCommand(
      "btw",
      "record a side note, kept out of the agent's context",
    );
    host.log("btw side-channel ready (/btw <note>)");
  },
  onCommand(name, args, host) {
    if (name !== "btw") return;
    const note = args.trim();
    if (!note) {
      host.note("usage: /btw <something to remember>");
      return;
    }
    notes.push(`[${stamp()}] ${note}`);
    host.note(`noted privately: ${note}`);
    host.setStatus("btw", `${notes.length} note(s)`);
    host.panel("BTW notes", notes.slice());
    host.log(`btw[${notes.length}] ${note}`);
  },
  onShutdown(host) {
    if (notes.length) host.log(`btw session notes:\n${notes.join("\n")}`);
  },
});
