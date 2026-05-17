# End-to-end DataTable on a REAL terminal: drive the unmodified
# `data_table_e2e` crossterm binary with real keystrokes — move the
# selection, toggle the sort, begin an in-cell edit, type a value, commit
# it, then filter — and assert the exact final `STATE …` line in
# data-table.expect. Proves the whole pipeline end to end on a real
# terminal: crossterm keys → the run loop → DataTableState + the reducer
# project() pipeline → a borrowed TextEdit written back into the data →
# the rendered frame. (Mouse hit-testing is covered by the deterministic
# DataTable::hit/cell_rect unit tests; this is the real-terminal key path,
# exactly as text-input.tape is for TextEdit.)
#
# Final state, computed exactly:
#   start: rows [Cy(s0), Ada(s1), Bo(s2)]; nothing sorted/selected
#   Down                 -> sel=1
#   "s"  toggle_sort(0)  -> sort 0:asc; visual=[Ada(s1),Bo(s2),Cy(s0)]
#                           sel=1 -> Bo (source 2)
#   "e"  begin_edit      -> editing (2,1), TextEdit empty
#   type "lead"          -> edit value "lead"
#   Enter commit         -> rows[2].role="lead"; commit=2,1=lead; reproject
#   "F"  filter "o"      -> keep Cy(s0:"ops"), Bo(s2:"bo"); drop Ada; rows=2
# => STATE sort=0:asc filter=o group=none sel=1 edit=none commit=2,1=lead rows=2

Output target/vhs/e2e/data-table.txt
Source vhs/common.tape
Set FontSize 14
Set Width 820
Set Height 600

Hide
Type "target/debug/examples/data_table_e2e"
Enter
Sleep 2s
Show

Sleep 800ms
Down
Sleep 350ms
Type "s"
Sleep 400ms
Type "e"
Sleep 350ms
Type "lead"
Sleep 350ms
Enter
Sleep 450ms
Type "F"
Sleep 1s
