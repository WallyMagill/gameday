#!/usr/bin/env bash
# Regenerate assets/logos/**/*.ans from ESPN CDN team PNGs via chafa.
# Dev-time only: the app ships the committed .ans files and never fetches art.
#
#   tools/gen-logos.sh              # default team set, 10x6 cells, sextants
#   SIZE=20x10 tools/gen-logos.sh   # bigger art
#   SYMBOLS=half tools/gen-logos.sh # pure half-blocks for fonts without sextants
set -euo pipefail
cd "$(dirname "$0")/.."

command -v chafa >/dev/null || { echo "chafa not found (brew install chafa)"; exit 1; }

SIZE="${SIZE:-10x6}"
SYMBOLS="${SYMBOLS:-sextant}"
TEAMS="${TEAMS:-nfl/kc nfl/tb nba/den nba/bos mlb/nyy mlb/tor nhl/edm nhl/dal}"

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
