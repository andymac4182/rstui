// TypeScript port of rstui-acp-plugin-ask-user (structured ask overlay).
import { definePlugin } from "../ts/index.mjs";

await definePlugin({
  onInit(_i, host) {
    host.registerCommand(
      "ask",
      "ask yourself a structured question (ask_user overlay)",
    );
  },
  async onCommand(name, args, host) {
    if (name !== "ask") return;
    const question = args.trim() || "How should we proceed?";
    const a = await host.askUser({
      question,
      context: "Answer routes back to the ask-user plugin, not the agent.",
      options: ["Yes, continue", "No, stop", "Let me explain (freeform)"],
      allowFreeform: true,
    });
    if (a.cancelled) {
      host.note("ask-user: cancelled");
      return;
    }
    const parts = [];
    if (a.selections.length) parts.push(`chose: ${a.selections.join(", ")}`);
    if (a.text) parts.push(`said: ${a.text}`);
    if (!parts.length) parts.push("no answer");
    host.note(`ask-user → ${parts.join(" · ")}`);
  },
});
