# End-to-end colour rendering on a REAL terminal: drive the unmodified
# `rstui-kitchen-sink` crossterm binary to the Colour Lab screen and assert
# its palette/markers render through the real terminal (colour-lab.expect).
# This is the real-terminal companion to the deterministic colour tests in
# rstui-smoke / rstui-kitchen-sink.

Output target/vhs/e2e/colour-lab.txt
Source vhs/common.tape
Set FontSize 13
Set Width 1080
Set Height 740

Hide
Type "target/debug/rstui-kitchen-sink"
Enter
Sleep 2s
Show

Sleep 1s
# Jump to the Colour Lab screen (hotkey 8).
Type "8"
Sleep 1200ms
# Move the swatch cursor so the index label changes — proves arrows reach
# the screen over a real terminal.
Right
Right
Down
Sleep 1200ms
