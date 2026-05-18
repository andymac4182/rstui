# End-to-end: drive the REAL crossterm `rstui-kitchen-sink` binary
# through the command palette into the json-render Agent-UI screen and
# settle on a deterministic final frame. `cargo xtask record e2e --check`
# asserts every marker in the sibling `agent-ui.expect` is present in the
# LAST frame.
#
# One palette nav from the boot (Welcome) screen — Welcome is *not* a
# text-entry screen, so `:` reliably opens the palette. (Both Agent-UI
# screens are editor text-entry screens; navigating *between* them in a
# tape would require escaping the editor, which is exactly the fragile
# path this gate deliberately avoids — the headless
# `experiences.rs` tests cover both screens + the editor directly.)
# The final frame is json-render example 1 ("Status card"): its rendered
# "Deployment" card / "api-gateway" heading text is static
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

# json-render — an agent's flat spec, editable ⇆ live projection.
Type ":"
Sleep 500ms
Type "json-render"
Sleep 700ms
Enter
Sleep 1800ms
