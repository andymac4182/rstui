#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

if ! command -v gnhf >/dev/null 2>&1; then
  echo "error: gnhf is not installed. Install it with: npm install -g gnhf" >&2
  exit 127
fi

real_claude="${CLAUDE_BIN:-}"
if [ -z "$real_claude" ]; then
  real_claude="$(command -v claude || true)"
fi

if [ -z "$real_claude" ]; then
  echo "error: claude is not installed or not on PATH. Install Claude Code and authenticate it first." >&2
  exit 127
fi

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rstui-gnhf.XXXXXX")"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

{
  printf '#!/usr/bin/env bash\n'
  printf 'set -euo pipefail\n'
  printf 'real_claude=%q\n' "$real_claude"
  cat <<'WRAPPER'
model="${RSTUI_CLAUDE_MODEL:-claude-opus-4-7}"
effort="${RSTUI_CLAUDE_EFFORT:-max}"
export CLAUDE_CODE_EFFORT_LEVEL="$effort"
exec "$real_claude" --model "$model" --effort "$effort" "$@"
WRAPPER
} >"$tmp_dir/claude"
chmod +x "$tmp_dir/claude"

cd "$repo_root"
PATH="$tmp_dir:$PATH" exec gnhf --agent claude "$@"
