#!/usr/bin/env bash
set -euo pipefail

stream="${1:-}"
main_repo="/Users/andrewmcclenaghan/dev/andymac4182/rstui"
worktree_root="/Users/andrewmcclenaghan/dev/andymac4182/rstui-stream-worktrees"

case "$stream" in
  widgets | fullscreen-runtime | rich-rendering | plugins | quality-dx)
    ;;
  *)
    echo "usage: $0 <widgets|fullscreen-runtime|rich-rendering|plugins|quality-dx>" >&2
    exit 2
    ;;
esac

goal_file="$main_repo/scripts/claude-goals/goal-conditions/$stream.md"
stamp="$(date +%Y%m%d-%H%M%S)"
worktree_name="rstui-$stream-$stamp"
branch="stream/$stream-$stamp"
worktree="$worktree_root/$worktree_name"
tmux_session="rstui_${stream}_${stamp}"

cd "$main_repo"

if [[ -n "$(git status --short)" ]]; then
  echo "rstui main checkout is dirty. Commit or stash changes before launching Claude streams." >&2
  git status --short >&2
  exit 1
fi

git fetch origin main
mkdir -p "$worktree_root"
git worktree add -b "$branch" "$worktree" main

if command -v pbcopy >/dev/null 2>&1; then
  pbcopy < "$goal_file"
  echo "Copied compact /goal condition to clipboard: $goal_file"
else
  echo "pbcopy not found. Paste this file into /goal manually: $goal_file"
fi

echo "Launching Claude stream '$stream' in explicit worktree: $worktree"
echo "In Claude, run /goal and paste the clipboard contents."

exec tmux new-session -A -s "$tmux_session" -c "$worktree" \
  "caffeinate -dimsu claude --model claude-opus-4-7 --effort max --dangerously-skip-permissions"
