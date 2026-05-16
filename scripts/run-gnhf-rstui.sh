#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

prompt_file="$script_dir/rstui-gnhf-prompt.md"
if [ ! -f "$prompt_file" ]; then
  echo "error: missing prompt file: $prompt_file" >&2
  exit 1
fi

prompt="$(<"$prompt_file")"

if [ "$#" -eq 0 ]; then
  set -- --current-branch --push
fi

exec "$script_dir/gnhf-claude-opus-max.sh" "$@" "$prompt"
