# rstui — shared VHS settings (the single source of truth).
#
# Every other tape `Source`s this file. VHS is always invoked with the
# working directory at the repository root (the `cargo xtask record` task
# and `scripts/record-demos.sh` both guarantee this), so `Source` and
# `Output` paths are repo-relative and stable.
#
# A resolution tape may re-`Set Width`/`Height`/`FontSize` AFTER sourcing
# this file; VHS applies `Set` in order, last value wins.
#
# NOTE: rendering needs `VHS_NO_SANDBOX=true` in this environment (the
# headless Chrome VHS drives cannot sandbox). The xtask task sets it.

Set Shell "bash"
Set Theme "Dracula"
Set FontSize 16
Set Padding 14
Set Margin 0
Set TypingSpeed 60ms
Set Width 1000
Set Height 620
