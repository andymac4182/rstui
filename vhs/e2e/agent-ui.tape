# End-to-end: drive the REAL crossterm `rstui-kitchen-sink` binary
# through BOTH Agent-UI screens — A2UI, then json-render — settling on a
# deterministic final frame. `cargo xtask record e2e --check` asserts
# every marker in the sibling `agent-ui.expect` is present in the LAST
# frame.
#
# Both screens are editor text-entry screens (the left pane owns every
# char key, so `:` types into the buffer, not the palette). This gate
# proves the real binary can still leave one editor screen for another:
# `Ctrl+K` opens the command palette deterministically even from a
# text-entry screen (it is bound in the `Capture::Text` context — the
# `⌘K palette` the footer advertises), so there is no fragile `Tab`
# dance. The final frame is json-render example 1 ("Status card"): its
# rendered "Deployment" card / "api-gateway" heading is static
# (tick-independent), so the assertion is stable.

Output target/vhs/e2e/agent-ui.txt
Source vhs/common.tape
Set FontSize 14
Set Width 1080
Set Height 740

Hide
Type "target/debug/rstui-kitchen-sink"
Enter
Sleep 2s
Show

Sleep 1s

# A2UI — an agent's A2UI v0.10 document, editable ⇆ live projection.
# (Welcome is not text-entry, so `:` opens the palette here.)
Type ":"
Sleep 500ms
Type "a2ui"
Sleep 700ms
Enter
Sleep 1400ms

# Ctrl+K opens the palette even though A2UI is an editor text-entry
# screen — deterministic, single chord, no Tab dance.
Ctrl+K
Sleep 600ms

# json-render — the deterministic final frame.
Type "json-render"
Sleep 700ms
Enter
Sleep 1800ms
