# End-to-end smoke: drive the REAL crossterm `rstui-kitchen-sink` binary to
# the observability suite via the command palette, and capture the final
# terminal text. `cargo xtask record e2e --check` asserts every marker in
# the sibling `observability-smoke.expect` file is present.
#
# This exercises the observability screens end to end through the real
# crossterm event/lifecycle path (not just the headless Harness): the
# palette jump, the screen render, and the chrome around it. The markers
# are stable header/panel literals, independent of the animation tick.

Output target/vhs/e2e/observability-smoke.txt
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
# command palette -> Observability screen
Type ":"
Sleep 500ms
Type "observability"
Sleep 800ms
Enter
Sleep 1500ms
# settle on a clean, deterministic final frame
Sleep 1s
