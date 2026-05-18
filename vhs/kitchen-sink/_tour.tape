# Shared kitchen-sink walkthrough, sourced by every resolution tape.
# The caller sets Output + Width/Height/FontSize, then Sources this.
# The kitchen sink reflows to whatever terminal size it is given, so the
# identical script demonstrates every resolution.

Hide
Type "target/debug/rstui-kitchen-sink"
Enter
Sleep 2s
Show

Sleep 1500ms

# Tour every screen by hotkey (1-8), pausing on each.
Type "2"
Sleep 1200ms
Tab
Sleep 400ms
Down 3
Sleep 900ms
Type "3"
Sleep 1200ms
Down 4
Sleep 800ms
Type "4"
Sleep 1400ms
Type "5"
Sleep 1400ms
Type "6"
Sleep 1400ms
Type "7"
Sleep 1400ms
Type "8"
Sleep 1400ms

# The observability suite (palette-reached: they sit past the digit hotkeys).
Type ":"
Sleep 400ms
Type "observability"
Sleep 700ms
Enter
Sleep 1600ms
Type ":"
Sleep 400ms
Type "metrics"
Sleep 700ms
Enter
Sleep 1600ms
Type ":"
Sleep 400ms
Type "traces"
Sleep 700ms
Enter
Sleep 1400ms
Type "f"
Sleep 1200ms

# The Agent-UI suite: an agent's A2UI document, editable on the left,
# its live rstui-jsonui projection on the right (ADR 0017).
Type ":"
Sleep 400ms
Type "a2ui"
Sleep 700ms
Enter
Sleep 1600ms
# Tab is a non-char key: it leaves the editor text-entry screen so the
# palette works again; then jump to the json-render screen.
Tab
Sleep 400ms
Type ":"
Sleep 400ms
Type "json-render"
Sleep 700ms
Enter
Sleep 1600ms

# Command palette: jump back to Forms by name.
Type ":"
Sleep 500ms
Type "forms"
Sleep 700ms
Enter
Sleep 1200ms

# Help overlay, then dismiss.
Type "?"
Sleep 1600ms
Escape
Sleep 500ms

# Settings drawer (theme toggle), then dismiss.
Type "g"
Sleep 1400ms
Escape
Sleep 500ms

# Home, then quit via the confirmation modal.
Type "1"
Sleep 1200ms
Type "q"
Sleep 900ms
Type "y"
Sleep 800ms
