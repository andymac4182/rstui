#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
prompt_file="$script_dir/prompts/quality-dx.md"

if [ ! -f "$prompt_file" ]; then
  echo "error: missing prompt file: $prompt_file" >&2
  exit 1
fi

if [ "$#" -eq 0 ]; then
  set -- --worktree --push
fi

exec "$script_dir/../gnhf-claude-opus-max.sh" "$@" "$(<"$prompt_file")"
