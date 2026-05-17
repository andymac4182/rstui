// Sample rstui-acp-client plugin (TypeScript SDK, JS form).
//
// A real plugin would `import { definePlugin } from "@rstui-acp/plugin-sdk"`
// (with the SDK installed). This sample uses a relative path so it runs with
// zero install (and powers the host smoke test).
//
// Run it:
//   rstui-acp-client --plugin "node sdk/v8-host/host.mjs sdk/examples/clock.plugin.mjs"
//
// It shows a footer clock, a /clock command that opens a modal, and a
// Ctrl+L keybinding bound to that command.

import { definePlugin } from "../ts/index.mjs";

function clock() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())} UTC`;
}

await definePlugin({
  onInit(_info, host) {
    host.registerCommand("clock", "Show a clock modal");
    host.registerKeybinding("ctrl+l", "clock", "Show the clock");
    host.footer([{ text: `🕐 ${clock()}`, fg: "black", bg: "cyan" }]);
    host.log("clock plugin (TypeScript SDK / V8 host) ready");
  },
  onTick(host) {
    host.footer([{ text: `🕐 ${clock()}`, fg: "black", bg: "cyan" }]);
  },
  async onCommand(name, _args, host) {
    if (name !== "clock") return;
    const choice = await host.modal(
      "Clock",
      [`The time is ${clock()}.`],
      ["Copy", "Close"],
    );
    host.note(choice === "Copy" ? `copied: ${clock()}` : "clock dismissed");
  },
});
