#!/usr/bin/env bash
# glyph-probe.sh — v3.3 Task 1 (glyph portability spike)
#
# Prints three labeled lines for visual comparison in a terminal screenshot:
#   (a) sextants (U+1FB00-1FB3B sample) — the Legacy Computing mosaic range
#       gameday's big digits and logo art currently emit.
#   (b) quadrants/half-blocks (U+2580-259F) — the fallback candidate range.
#   (c) a real captured digit/logo row pulled from out/board-broadcast.ansi
#       (regenerate with `cargo run --release -- dump` if out/ is empty —
#       ANSI text dumps don't need fonts to produce).
#
# Also prints $TERM_PROGRAM / $TERM so each screenshot is self-labeling.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANSI_FILE="$REPO_ROOT/out/board-broadcast.ansi"

echo "TERM_PROGRAM=${TERM_PROGRAM:-<unset>}  TERM=${TERM:-<unset>}"
echo
echo "(a) sextants   : 🬀🬁🬂🬃🬄🬅🬆🬇 🬐🬑🬒 🬭🬮🬯"
echo "(b) quadrants  : ▀▁▂▃▄▅▆▇█ ▖▗▘▙▚▛▜▝▞▟"

if [ -f "$ANSI_FILE" ]; then
  # First line containing a sextant glyph (U+1FB00-1FB3B), ANSI color codes stripped.
  # (BSD grep has no -P/PCRE, so use perl for the Unicode range match.)
  ROW="$(perl -CSD -ne 'if (/[\x{1FB00}-\x{1FB3B}]/) { s/\e\[[0-9;]*m//g; print; exit }' "$ANSI_FILE")"
  echo "(c) captured   : $ROW"
else
  echo "(c) captured   : <missing> — run: cargo run --release -- dump"
fi
