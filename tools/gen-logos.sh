#!/usr/bin/env bash
# Regenerate assets/logos/**/*.ans from ESPN's team art via chafa, then
# rewrite the embed table `src/board/logo_sources.rs`.
# Dev-time only: the app ships the committed .ans files and never fetches art.
#
#   tools/gen-logos.sh                 # every league, 16x10 cells, quadrants
#   LEAGUES="nba epl" tools/gen-logos.sh   # just these
#   SIZE=20x12 tools/gen-logos.sh      # bigger art
#   SYMBOLS=sextant tools/gen-logos.sh # denser, but tofus on Terminal.app
#
# v3.4 §7: the URL is no longer guessed. v3.3 built
# `teamlogos/$league/500/$abbr.png`, which is only true for the five US pro
# leagues — college and soccer art is id-keyed (`ncaa/500/194.png`,
# `soccer/500/364.png`) and every college/soccer guess 404'd. So each league's
# `/teams?limit=1000` payload is read instead and the `logos[]` entry tagged
# `rel: ["full","default"]` is what gets fetched. That is ESPN telling us the
# URL rather than us inventing one, and it is why the key scheme in
# `domain::logo_key` is `ncaa/<id>` and `soccer/<id>` for those leagues.
#
# WHEN TO RERUN
# - EPL churn: three clubs are relegated and three promoted every summer, so
#   the 20 committed `soccer/*` EPL marks go stale once a season. Rerun after
#   promotion is settled; the departed clubs' marks are harmless (nothing
#   looks them up) but the arrivals have none until you do.
# - Poll refresh: college ships the ranked top 25 only (761 FBS + 362 D-I
#   schools is not a bundle worth carrying). The AP poll moves weekly in
#   season, so a school that climbs in has no mark until the next run.
#   Unranked schools fall back to the abbreviation mark — that is the
#   designed behaviour, not a gap: `board::logo::hero_mark` returns None and
#   `draw_abbr_mark` paints the abbreviation in team colours.
set -euo pipefail
cd "$(dirname "$0")/.."

command -v chafa >/dev/null || { echo "chafa not found (brew install chafa)"; exit 1; }
command -v jq >/dev/null || { echo "jq not found (brew install jq)"; exit 1; }

# 16x10 is the committed hero-mark size (`board::logo`, and the 18-col flank
# floor in `board::hero`). Quadrant + half blocks only: ruling R41 — the v3.2
# marks were generated with `sextant`, whose U+1FB00-1FB3B range Terminal.app's
# default font has no coverage for, so every mark rendered as a field of tofu
# boxes there (and in the gallery's own PNG pipeline). The quadrant range
# (U+2580-259F) is the oldest, widest-covered block run in Unicode; it costs
# some detail and gives back art that draws everywhere `█` draws.
SIZE="${SIZE:-16x10}"
SYMBOLS="${SYMBOLS:-space+solid+half+quad}"
API="https://site.api.espn.com/apis/site/v2/sports"
LEAGUES="${LEAGUES:-nfl nba wnba nhl mlb epl mls cfb cbb}"

# Downloads survive between runs: ~190 PNGs is a lot to re-pull from a CDN
# just to retune chafa flags. Delete the dir to force a refetch.
CACHE="${LOGO_CACHE:-${TMPDIR:-/tmp}/gameday-logo-cache}"
mkdir -p "$CACHE"

# One line per league: slug sport competition namespace key-by
# `key-by` is `id` where ESPN's art is id-keyed (college, soccer) and `abbr`
# where it is abbreviation-keyed (the US pro leagues); `poll` means "read the
# rankings endpoint, take the top 25" instead of the whole teams list.
league_spec() {
  case "$1" in
    nfl)  echo "football nfl nfl abbr" ;;
    nba)  echo "basketball nba nba abbr" ;;
    wnba) echo "basketball wnba wnba abbr" ;;
    nhl)  echo "hockey nhl nhl abbr" ;;
    mlb)  echo "baseball mlb mlb abbr" ;;
    epl)  echo "soccer eng.1 soccer id" ;;
    mls)  echo "soccer usa.1 soccer id" ;;
    cfb)  echo "football college-football ncaa poll" ;;
    cbb)  echo "basketball mens-college-basketball ncaa poll" ;;
    *) return 1 ;;
  esac
}

skipped=()
written=0

# key<TAB>href for every team in a league, off ESPN's own logos[] rel tags.
resolve_league() {
  local sport="$1" comp="$2" ns="$3" keyby="$4" url json
  if [ "$keyby" = poll ]; then
    # `curatedRank <= 25` rides scoreboard competitors, not the teams
    # endpoint — and only for teams that happen to be playing that day. The
    # rankings endpoint is the same poll, complete, in one request, and it
    # carries the team's logos[] inline, so it is both the honest and the
    # cheapest source for "who is in the top 25".
    url="$API/$sport/$comp/rankings"
    json=$(curl -fsSL --max-time 30 "$url") || { echo "SKIP league $comp: rankings fetch failed" >&2; return 0; }
    jq -r --arg ns "$ns" '
      ([.rankings[] | select(.shortName | test("AP"; "i"))] + .rankings)[0]
      | .ranks[]? | select(.current >= 1 and .current <= 25)
      | [ "\($ns)/\(.team.id)",
          ([.team.logos[]? | select((.rel|index("full")) and (.rel|index("default"))) | .href] | first // "")
        ] | @tsv' <<<"$json"
  else
    url="$API/$sport/$comp/teams?limit=1000"
    json=$(curl -fsSL --max-time 30 "$url") || { echo "SKIP league $comp: teams fetch failed" >&2; return 0; }
    jq -r --arg ns "$ns" --arg keyby "$keyby" '
      .sports[0].leagues[0].teams[].team
      | [ (if $keyby == "id" then "\($ns)/\(.id)" else "\($ns)/\(.abbreviation | ascii_downcase)" end),
          ([.logos[]? | select((.rel|index("full")) and (.rel|index("default"))) | .href] | first // "")
        ] | @tsv' <<<"$json"
  fi
}

render() {
  local key="$1" href="$2" png cached lum
  if [ -z "$href" ]; then
    skipped+=("$key (no full/default logo in the payload)")
    return 0
  fi
  cached="$CACHE/${key//\//-}.png"
  if [ ! -s "$cached" ]; then
    if ! curl -fsSL --max-time 30 "$href" -o "$cached"; then
      rm -f "$cached"
      skipped+=("$key (fetch failed: $href)")
      return 0
    fi
    sleep 0.15  # ~190 marks in one run; don't hammer the CDN
  fi
  png="$tmp/${key//\//-}.png"
  cp "$cached" "$png"
  # Dark marks (navy Yankees, black-green Stars) go murky on a black board;
  # lift brightness when mean luminance over black is low. 0.18 chosen from
  # measuring the demo set: dark marks sit ~0.10, normal ones >= 0.24.
  if command -v magick >/dev/null; then
    # ESPN art ships with transparent padding; trimming it buys resolution.
    magick "$png" -trim +repage "$png" 2>/dev/null || { skipped+=("$key (not a usable image)"); return 0; }
    lum=$(magick "$png" -background black -alpha remove -colorspace gray -format "%[fx:mean]" info:)
    if awk "BEGIN{exit !($lum < 0.18)}"; then
      magick "$png" -modulate 145,115 "$png"
    fi
  fi
  mkdir -p "assets/logos/${key%/*}"
  if ! chafa --size="$SIZE" --symbols="$SYMBOLS" -c full -w 9 -f symbols --bg black "$png" \
       > "assets/logos/$key.ans"; then
    rm -f "assets/logos/$key.ans"
    skipped+=("$key (chafa failed)")
    return 0
  fi
  written=$((written + 1))
  echo "wrote assets/logos/$key.ans ($(wc -l < "assets/logos/$key.ans" | tr -d ' ') rows)"
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

seen="$tmp/seen"
: > "$seen"
for l in $LEAGUES; do
  spec=$(league_spec "$l") || { echo "unknown league: $l" >&2; exit 1; }
  # shellcheck disable=SC2086
  set -- $spec
  echo "== $l ($1/$2 -> $3, by $4)"
  while IFS=$'\t' read -r key href; do
    [ -n "$key" ] || continue
    # ncaa is one bucket for CFB and CBB: a school ranked in both polls
    # carries one id and one mark. Render it once.
    grep -qxF "$key" "$seen" && continue
    echo "$key" >> "$seen"
    render "$key" "$href"
  done < <(resolve_league "$1" "$2" "$3" "$4")
done

# The embed table is generated, never hand-edited: `include_str!` needs
# literal paths, and 190 of them is not a list a human keeps in sync.
out=src/board/logo_sources.rs
{
  echo "//! @generated by \`tools/gen-logos.sh\` — do not edit by hand."
  echo "//!"
  echo "//! Every committed hero mark, embedded in the binary. Keys follow"
  echo "//! [\`crate::domain::logo_key\`]: \`<league>/<abbr>\` for the US pro"
  echo "//! leagues, \`ncaa/<id>\` and \`soccer/<id>\` where ESPN's art is."
  echo
  echo "pub(super) const LOGO_SOURCES: &[(&str, &str)] = &["
  find assets/logos -name '*.ans' | sort | while read -r f; do
    key=${f#assets/logos/}
    key=${key%.ans}
    echo "    (\"$key\", include_str!(\"../../$f\")),"
  done
  echo "];"
} > "$out"
echo "wrote $out ($(grep -c include_str "$out") marks)"

echo
echo "rendered $written mark(s); ${#skipped[@]} skipped"
for s in ${skipped+"${skipped[@]}"}; do echo "  SKIP $s"; done
