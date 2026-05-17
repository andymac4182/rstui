# End-to-end: drive the REAL crossterm `rstui-kitchen-sink` binary through
# the command palette into every observability / data-viz screen, then settle
# on a deterministic final frame. `cargo xtask record e2e --check` asserts
# every marker in the sibling `kitchen-sink-dataviz.expect` is present.
#
# These five screens (Dashboard, Live Logs, Observability, Metrics, Traces)
# have no number hotkey — they are palette-only — so this is the only e2e
# that proves the real binary can boot, translate palette input, leave a
# text-entry screen, and render the observability widget family end to end.
# The final frame is Metrics: its "Latency heatmap" panel title is static
# (tick-counter independent), so the assertion is stable.

Output target/vhs/e2e/kitchen-sink-dataviz.txt
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

# Dashboard (KPIs, charts, activity)
Type ":"
Sleep 500ms
Type "dashboard"
Sleep 600ms
Enter
Sleep 1200ms

# Observability (golden signals, throughput vs errors)
Type ":"
Sleep 500ms
Type "observability"
Sleep 600ms
Enter
Sleep 1200ms

# Traces (span waterfall ⇄ flame graph)
Type ":"
Sleep 500ms
Type "traces"
Sleep 600ms
Enter
Sleep 1200ms

# Live Logs (a text-entry screen: the filter owns char keys)
Type ":"
Sleep 500ms
Type "live logs"
Sleep 600ms
Enter
Sleep 1200ms

# Tab is a non-char key: it flows past the Logs filter to the global keymap
# and moves focus to the sidebar, so the palette ":" works again.
Tab
Sleep 500ms

# Metrics — the deterministic final frame ("Latency heatmap" is static).
Type ":"
Sleep 500ms
Type "metrics"
Sleep 600ms
Enter
Sleep 1500ms
