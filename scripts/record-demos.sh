#!/usr/bin/env bash
# Regenerate every documentation recording with VHS.
#
# Thin human-facing wrapper around `cargo xtask record`. It exports the
# VHS_NO_SANDBOX flag the headless browser needs in this environment and
# always runs from the repository root so tape paths resolve.
#
# Usage:
#   scripts/record-demos.sh                # everything
#   scripts/record-demos.sh widgets        # just the widget GIFs
#   scripts/record-demos.sh kitchen-sink   # the 4 resolution videos
#   scripts/record-demos.sh gallery        # the hero GIF
#   scripts/record-demos.sh e2e            # capture the e2e walkthrough
#   scripts/record-demos.sh e2e --check    # e2e regression gate (CI-style)
#
# Requires: vhs, ttyd, ffmpeg  ->  brew install vhs ttyd ffmpeg
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if ! command -v vhs >/dev/null 2>&1; then
  echo "record-demos: vhs not found. Install it: brew install vhs ttyd ffmpeg" >&2
  exit 1
fi

export VHS_NO_SANDBOX=true
exec cargo run -q -p xtask -- record "$@"
