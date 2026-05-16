#!/usr/bin/env bash
set -euo pipefail

stream="${1:-}"
repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"

case "$stream" in
  widgets | fullscreen-runtime | rich-rendering | plugins | quality-dx)
    ;;
  *)
    echo "usage: $0 <widgets|fullscreen-runtime|rich-rendering|plugins|quality-dx>" >&2
    exit 2
    ;;
esac

goal_file="$repo/scripts/claude-goals/goal-conditions/$stream.md"
worktree_name="rstui-$stream-$(date +%Y%m%d-%H%M%S)"

cd "$repo"

if [[ -n "$(git status --short)" ]]; then
  echo "rstui main checkout is dirty. Commit or stash changes before launching Claude streams." >&2
  git status --short >&2
  exit 1
fi

if command -v pbcopy >/dev/null 2>&1; then
  pbcopy < "$goal_file"
  echo "Copied compact /goal condition to clipboard: $goal_file"
else
  echo "pbcopy not found. Paste this file into /goal manually: $goal_file"
fi

echo "Launching Claude stream '$stream' in worktree '$worktree_name'."
echo "In Claude, run /goal and paste the clipboard contents."

exec caffeinate -dimsu claude \
  --worktree "$worktree_name" \
  --tmux \
  --add-dir "$repo" \
  --model claude-opus-4-7 \
  --effort max \
  --dangerously-skip-permissions
