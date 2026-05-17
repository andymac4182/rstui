# End-to-end text input on a REAL terminal: drive the unmodified
# `text_input_e2e` crossterm binary with real keystrokes — Left/Right
# arrows, Backspace, Delete, and the Ctrl+A / Ctrl+E / shortcuts (which the
# fixture maps to Home/End) — then assert the final `STATE …` line in
# text-input.expect. VHS has no bare Home/End key, so the Ctrl shortcuts
# (real modifier combinations over a real terminal) stand in for them — a
# stronger test of "modifiers on inputs" anyway. Bare Home/End KeyCodes are
# covered by the crossterm matrix + the deterministic Harness e2e.
#
# Final state, computed exactly:
#   ""              type "hello"   -> "hello" cur5
#   Ctrl+A (Home)                  -> cur0
#   type "X"                       -> "Xhello" cur1
#   Ctrl+E (End)                   -> cur6
#   Backspace                      -> "Xhell" cur5  (deletes 'o')
#   Left Left                      -> cur3
#   Delete                         -> "Xhel" cur3   (deletes 'l' at index 3)
#   Ctrl+A (Home)                  -> cur0
#   Ctrl+E (End)                   -> cur4
# => STATE val=Xhel cur=4 len=4

Output target/vhs/e2e/text-input.txt
Source vhs/common.tape
Set FontSize 16
Set Width 900
Set Height 360

Hide
Type "target/debug/examples/text_input_e2e"
Enter
Sleep 2s
Show

Sleep 800ms
Type "hello"
Sleep 400ms
Ctrl+A
Sleep 250ms
Type "X"
Sleep 250ms
Ctrl+E
Sleep 250ms
Backspace
Sleep 250ms
Left
Left
Sleep 250ms
Delete
Sleep 250ms
Ctrl+A
Sleep 250ms
Ctrl+E
Sleep 900ms
