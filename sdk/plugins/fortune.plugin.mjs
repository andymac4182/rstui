// TypeScript port of rstui-acp-plugin-fortune (rotating dev fortune).
import { definePlugin } from "../ts/index.mjs";

const FORTUNES = [
  "Make it work, make it right, make it fast.",
  "Weeks of coding can save you hours of planning.",
  "There are two hard things: cache invalidation and naming.",
  "The best code is the code you didn't have to write.",
  "Premature optimization is the root of all evil.",
  "Programs must be written for people to read.",
  "Simplicity is prerequisite for reliability.",
  "First, solve the problem. Then, write the code.",
  "Deleted code is debugged code.",
  "A good agent reads the diff before it trusts the patch.",
  "Talk is cheap. Show me the tests.",
  "If it hurts, do it more often — automate the pain away.",
];

let idx = 0;
function draw(host) {
  const f = FORTUNES[idx % FORTUNES.length];
  idx += 1;
  host.note(`🥠 ${f}`);
  host.panel("Fortune", [f]);
}

await definePlugin({
  onInit(_i, host) {
    host.registerCommand("fortune", "Draw a developer fortune");
    host.registerKeybinding("ctrl+y", "fortune", "Draw a fortune");
  },
  onTurnEnded: (_r, host) => draw(host),
  onCommand(name, _args, host) {
    if (name === "fortune") draw(host);
  },
});
