#!/usr/bin/env bash
# Regenerate assets/logos/**/*.ans from ESPN CDN team PNGs via chafa.
# Dev-time only: the app ships the committed .ans files and never fetches art.
#
#   tools/gen-logos.sh              # default team set, 16x10 cells, quadrants
#   SIZE=20x12 tools/gen-logos.sh   # bigger art
#   SYMBOLS=sextant tools/gen-logos.sh  # denser, but tofus on Terminal.app
set -euo pipefail
cd "$(dirname "$0")/.."

command -v chafa >/dev/null || { echo "chafa not found (brew install chafa)"; exit 1; }

# 16x10 is the committed hero-mark size (`board::logo`, and the 18-col flank
# floor in `board::hero`). Quadrant + half blocks only: ruling R41 — the v3.2
# marks were generated with `sextant`, whose U+1FB00-1FB3B range Terminal.app's
# default font has no coverage for, so every mark rendered as a field of tofu
# boxes there (and in the gallery's own PNG pipeline). The quadrant range
# (U+2580-259F) is the oldest, widest-covered block run in Unicode; it costs
# some detail and gives back art that draws everywhere `█` draws.
SIZE="${SIZE:-16x10}"
SYMBOLS="${SYMBOLS:-space+solid+half+quad}"
# All 32 NFL teams (ESPN abbrs) + the non-NFL demo set. Other leagues fall
# back to abbreviation marks until their sets are generated.
NFL_ALL="ari atl bal buf car chi cin cle dal den det gb hou ind jax kc lv lac lar mia min ne no nyg nyj phi pit sea sf tb ten wsh"
DEFAULT_TEAMS="$(for a in $NFL_ALL; do printf 'nfl/%s ' "$a"; done)nba/den nba/bos mlb/nyy mlb/tor nhl/edm nhl/dal"
TEAMS="${TEAMS:-$DEFAULT_TEAMS}"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for t in $TEAMS; do
  league=${t%/*}
  abbr=${t#*/}
  png="$tmp/$league-$abbr.png"
  curl -fsSL "https://a.espncdn.com/i/teamlogos/$league/500/$abbr.png" -o "$png"
  # Dark marks (navy Yankees, black-green Stars) go murky on a black board;
  # lift brightness when mean luminance over black is low. 0.18 chosen from
  # measuring the demo set: dark marks sit ~0.10, normal ones >= 0.24.
  if command -v magick >/dev/null; then
    # ESPN art ships with transparent padding; trimming it buys resolution.
    magick "$png" -trim +repage "$png"
    lum=$(magick "$png" -background black -alpha remove -colorspace gray -format "%[fx:mean]" info:)
    if awk "BEGIN{exit !($lum < 0.18)}"; then
      magick "$png" -modulate 145,115 "$png"
    fi
  fi
  mkdir -p "assets/logos/$league"
  chafa --size="$SIZE" --symbols="$SYMBOLS" -c full -w 9 -f symbols --bg black "$png" \
    > "assets/logos/$league/$abbr.ans"
  echo "wrote assets/logos/$league/$abbr.ans ($(wc -l < "assets/logos/$league/$abbr.ans" | tr -d ' ') rows)"
done
