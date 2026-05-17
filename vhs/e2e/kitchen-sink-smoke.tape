# End-to-end smoke: drive the REAL crossterm `rstui-kitchen-sink` binary
# through a deterministic keystroke script and capture the final terminal
# text. `cargo xtask record e2e --check` asserts every marker in the
# sibling `kitchen-sink-smoke.expect` file is present in the last frame.
#
# This exercises what the headless Harness tests cannot: crossterm event
# translation, the panic-safe lifecycle, real terminal sizing — end to end.
# The markers are header/footer/screen literals that are stable regardless
# of the animation tick counter (taken from the kitchen-sink test suite).

Output target/vhs/e2e/kitchen-sink-smoke.txt
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
# number key -> Forms screen
Type "2"
Sleep 1s
# open + close the help overlay (exercises an overlay end to end)
Type "?"
Sleep 1200ms
Escape
Sleep 800ms
# settle on a clean, deterministic final frame (Forms screen)
Sleep 1s
