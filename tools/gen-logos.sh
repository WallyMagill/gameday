#!/usr/bin/env bash
# Regenerate assets/logos/**/*.ans (dark ground) and assets/logos-light/**/*.ans
# (light ground) from ESPN's team art via chafa, then rewrite the embed table
# `src/board/logo_sources.rs`.
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

# COLLEGE=all (default; the shipped set, wave 5 direction 10): every FBS
# team (cfb) plus the top eight D-I men's basketball conferences (cbb) —
# COLLEGE=poll is the older ranked-25 set — cfb/cbb rows from the AP/coaches
# rankings endpoint, top 25 only (`resolve_league` below). The full set —
# ACC, Big East, Big Ten, Big 12, SEC, Atlantic 10, Mountain West, American
# — off the standings payloads `provider::espn::standings_url` and
# `provider::map::map_standings` already read, not the teams list (the
# addendum: `.../teams?groups=80` returns all 761 college teams, not FBS).
# Verified 2026-09-07 against `.../college-football/standings?group=80`
# (138 FBS teams, 11 conferences) and `.../mens-college-basketball/
# standings?group=50` (all D-I, 31 conferences, filtered to the eight
# named above: 118 teams). ESPN keys a school by one id across both
# sports, so the existing `seen` dedupe collapses the overlap (measured:
# 92 of the 118 hoops ids already appear in the FBS 138) down to 165
# unique `ncaa/<id>` marks (124 net new over the poll set) — see
# `resolve_college_all`. Binary cost is `include_str!`, so it is paid once
# at compile time: measured +0.6 MB in `target/release/gameday` (6.6 MB ->
# 7.2 MB, both grounds; task report). One-time dev fetch (`COLLEGE=all LEAGUES="cfb cbb"
# tools/gen-logos.sh`); the shipped binary embeds whatever is committed
# under `assets/logos*`, poll or all, with no runtime cost either way.
COLLEGE="${COLLEGE:-all}"   # the shipped set (wave 5 direction 10); COLLEGE=poll for the ranked-25 set

# ---------------------------------------------------------- the light set
# v3.4 §7. Every mark is rendered twice: once over black (the dark themes)
# and once over daygame's paper ground (`assets/candidates/daygame.toml`,
# bg = #f5f2ea). One art set cannot serve both — a gold Pirates P is a
# perfect mark on black and an invisible one on paper.
PAPER="${PAPER:-#f5f2ea}"
# A pixel clears the WCAG 3:1 graphics floor against the paper when its
# relative luminance is <= 0.263: contrast = (0.889 + 0.05) / (L + 0.05) = 3
# with the paper's L = 0.889 (the receipt in daygame.toml).
#
# That 0.263 is *linear* light, which ImageMagick's plain `-colorspace gray`
# does not give: it computes Rec709 luma over the gamma-encoded channels
# (`xc:#e31837 -colorspace gray` measures 0.272 where the relative luminance
# is 0.171), which reads saturated brand colors as far darker than they are —
# the difference between flagging a gold Pirates P and shipping it invisible.
# `-evaluate Pow 2.2` first puts the channels in linear light, and then gray
# is the luminance WCAG means (checked: #f5f2ea -> 0.892, #808080 -> 0.220).
READABLE_LUMA="${READABLE_LUMA:-26.3%}"
# A mark is "contrast-hostile on paper" when the MAJORITY of its own opaque
# pixels miss that floor — the median pixel of the mark is illegible. Half is
# the one non-arbitrary place to put that line, and it is what asks ESPN for
# its on-white art: a swap that costs nothing, since the answer is the team's
# own logo either way. 40 of the committed 230 fall below it; for 15 of them
# ESPN's on-white file actually measures better and is taken.
READABLE_SHARE="${READABLE_SHARE:-0.5}"
# Darkening is not free — it moves a brand colour — so the lift waits for a
# lower line: less than a fifth of the mark reading, where what is left is
# speckle rather than shape. The fifth comes off the gate frame by eye, not
# out of arithmetic: the KC arrowhead reads at 0.39 and is a perfectly legible
# arrowhead on paper (lift it and its white interior turns gray, which is
# worse art), while the Pirates' gold P reads at 0.00 and is a ghost. Eight
# marks are under it: mlb/pit, mlb/sf and ncaa/2633 at 0.00, nfl/pit 0.15,
# ncaa/130 0.16, nfl/ten 0.16, nba/phx 0.17, soccer/362 0.19.
LIFT_SHARE="${LIFT_SHARE:-0.2}"

# Downloads survive between runs: 230 PNGs (plus the on-white variants the
# light set asks for) is a lot to re-pull from a CDN just to retune chafa
# flags. Delete the dir to force a refetch.
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
written_light=0
subs=()
stubborn=()

# key<TAB>href<TAB>on-white-href for every team in a league, off ESPN's own
# logos[] rel tags. The third column is the `primary_logo_on_white_color`
# variant — ESPN's own answer to "this mark is going on white paper". It is
# published for every US league and for MLS, and for nobody in the EPL
# (checked 2026-09-03: 0 of 20 clubs), so it is a column that is often empty
# and the light path has to survive that.
# COLLEGE=all's source: the standings payloads, not the rankings poll.
# `site.web.api.espn.com` (not `site.api.espn.com` — the two hosts serve
# different shapes for this endpoint; the app's own `standings_url` is
# the receipt) `/apis/v2/sports/$sport/$comp/standings?group=$group`.
STANDINGS_API="https://site.web.api.espn.com/apis/v2/sports"
# The brief's eight, matched against ESPN's own `abbreviation` field on
# each conference node (case as ESPN returns it — checked, not guessed).
CBB_CONFS="acc bige big10 big12 sec atl10 mwest American"

resolve_college_all() {
  local sport="$1" comp="$2" ns="$3" group url json
  case "$comp" in
    college-football) group=80 ;;
    mens-college-basketball) group=50 ;;
    *) echo "COLLEGE=all has no group mapping for $comp" >&2; return 1 ;;
  esac
  url="$STANDINGS_API/$sport/$comp/standings?group=$group"
  json=$(curl -fsSL --max-time 30 "$url") || { echo "SKIP league $comp: standings fetch failed" >&2; return 0; }
  if [ "$comp" = college-football ]; then
    local n
    n=$(jq '[.children[].standings.entries[].team] | length' <<<"$json")
    echo "  college(all) $comp: FBS group $group, $n teams" >&2
  else
    jq -r --arg confs "$CBB_CONFS" '
      ($confs | split(" ")) as $want
      | .children[] | select(.abbreviation as $a | $want | index($a))
      | "  \(.name) (\(.abbreviation)): \(.standings.entries | length) teams"
    ' <<<"$json" >&2
  fi
  jq -r --arg ns "$ns" --arg confs "$CBB_CONFS" --arg comp "$comp" '
    (if $comp == "college-football"
     then [.children[].standings.entries[].team]
     else ($confs | split(" ")) as $want
        | [.children[] | select(.abbreviation as $a | $want | index($a)) | .standings.entries[].team]
     end)
    | .[]
    | [ "\($ns)/\(.id)",
        ([.logos[]? | select((.rel|index("full")) and (.rel|index("default"))) | .href] | first // ""),
        ([.logos[]? | select(.rel|index("primary_logo_on_white_color")) | .href] | first // "")
      ] | @tsv' <<<"$json"
}

resolve_league() {
  local sport="$1" comp="$2" ns="$3" keyby="$4" url json
  if [ "$keyby" = poll ] && [ "$COLLEGE" = all ]; then
    resolve_college_all "$sport" "$comp" "$ns"
    return
  fi
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
          ([.team.logos[]? | select((.rel|index("full")) and (.rel|index("default"))) | .href] | first // ""),
          ([.team.logos[]? | select(.rel|index("primary_logo_on_white_color")) | .href] | first // "")
        ] | @tsv' <<<"$json"
  else
    url="$API/$sport/$comp/teams?limit=1000"
    json=$(curl -fsSL --max-time 30 "$url") || { echo "SKIP league $comp: teams fetch failed" >&2; return 0; }
    jq -r --arg ns "$ns" --arg keyby "$keyby" '
      .sports[0].leagues[0].teams[].team
      | [ (if $keyby == "id" then "\($ns)/\(.id)" else "\($ns)/\(.abbreviation | ascii_downcase)" end),
          ([.logos[]? | select((.rel|index("full")) and (.rel|index("default"))) | .href] | first // ""),
          ([.logos[]? | select(.rel|index("primary_logo_on_white_color")) | .href] | first // "")
        ] | @tsv' <<<"$json"
  fi
}

# Cache-or-download; echoes the cached path. `$3` distinguishes the variants
# of one key in the cache ("" for the default mark, "-onwhite" for ESPN's).
fetch() {
  local key="$1" href="$2" suffix="${3:-}" cached
  cached="$CACHE/${key//\//-}$suffix.png"
  if [ ! -s "$cached" ]; then
    if ! curl -fsSL --max-time 30 "$href" -o "$cached"; then
      rm -f "$cached"
      return 1
    fi
    sleep 0.15  # 230 marks in one run; don't hammer the CDN
  fi
  printf '%s' "$cached"
}

# The share of a mark's own (opaque) pixels that clear 3:1 against the paper.
# Alpha-masked on purpose: the transparent field around a mark is the board's
# ground, not the mark, and counting it would score every logo by how much
# padding ESPN shipped.
readable_share() {
  local png="$1" both alpha
  magick "$png" -alpha extract -threshold 50% "$tmp/mask.png"
  magick "$png" -background "$PAPER" -alpha remove -alpha off \
    -evaluate Pow 2.2 -colorspace gray -threshold "$READABLE_LUMA" -negate "$tmp/legible.png"
  both=$(magick "$tmp/mask.png" "$tmp/legible.png" -compose multiply -composite \
    -format "%[fx:mean]" info:)
  alpha=$(magick "$tmp/mask.png" -format "%[fx:mean]" info:)
  awk -v b="$both" -v a="$alpha" 'BEGIN { if (a <= 0) print "0.000"; else printf "%.3f", b / a }'
}

# The lookup key is the team abbreviation; the file name is not always allowed
# to be. CON (the WNBA Sun) is a reserved DOS device name, so `con.ans` cannot
# exist in a Windows checkout whatever the extension — that mark is stored as
# `con_.ans`. These two map between the key and its file stem, in both
# directions, so the embed table keeps saying "wnba/con".
file_stem() { case "$1" in */con) echo "${1}_" ;; *) echo "$1" ;; esac; }
stem_key() { case "$1" in */con_) echo "${1%_}" ;; *) echo "$1" ;; esac; }

# The same mark, composited for daygame's paper instead of the dark themes'
# black: ESPN's on-white variant where the standard one is contrast-hostile,
# then the v3.3 brightness lift run in reverse.
render_light() {
  local key="$1" cached="$2" white="$3" png share share_white white_png
  png="$tmp/light-${key//\//-}.png"
  cp "$cached" "$png"
  if command -v magick >/dev/null; then
    magick "$png" -trim +repage "$png" 2>/dev/null || {
      skipped+=("$key light (not a usable image)")
      return 0
    }
    share=$(readable_share "$png")
    if awk "BEGIN{exit !($share < $READABLE_SHARE)}"; then
      # Contrast-hostile on paper. ESPN publishes a mark drawn for white
      # grounds; take it when it measures better, never on faith — for most
      # teams the two files are the same art and swapping buys nothing.
      if [ -n "$white" ] && white_png=$(fetch "$key" "$white" -onwhite) \
        && magick "$white_png" -trim +repage "$tmp/onwhite.png" 2>/dev/null; then
        share_white=$(readable_share "$tmp/onwhite.png")
        if awk "BEGIN{exit !($share_white > $share)}"; then
          cp "$tmp/onwhite.png" "$png"
          subs+=("$key  on-white variant  readable share $share -> $share_white")
          share=$share_white
        else
          subs+=("$key  kept standard, on-white no better ($share vs $share_white)")
        fi
      else
        subs+=("$key  kept standard, no primary_logo_on_white_color published (share $share)")
      fi
      # The v3.3 lift, mirrored. Dark marks over black got `-modulate 145,115`
      # because black's failing tail is the dark one; paper's failing tail is
      # the bright one, so the mirror divides where the original multiplied:
      # 100 * 100 / 145 = 69, with the same +15% saturation so the hue
      # survives the move (a gold Pirates P goes to a dark gold, not to gray).
      if awk "BEGIN{exit !($share < $LIFT_SHARE)}"; then
        magick "$png" -modulate 69,115 "$png"
        local after
        after=$(readable_share "$png")
        subs+=("$key  darkened 69,115  readable share $share -> $after")
        # One pass, like the dark path's one lift. What is still short of the
        # floor here is short by a hair (mlb/pit's gold lands at 0.280 linear
        # luminance against the 0.263 the floor wants — 2.85:1, not 3:1) or is
        # a mark with no dark tone to find; darkening it further would buy the
        # ratio by turning a brand color to mud.
        if awk "BEGIN{exit !($after < $LIFT_SHARE)}"; then
          stubborn+=("$key ($after)")
        fi
      fi
    fi
  fi
  local stem
  stem=$(file_stem "$key")
  mkdir -p "assets/logos-light/${key%/*}"
  if ! chafa --size="$SIZE" --symbols="$SYMBOLS" -c full -w 9 -f symbols --bg "$PAPER" "$png" \
    > "assets/logos-light/$stem.ans"; then
    rm -f "assets/logos-light/$stem.ans"
    skipped+=("$key light (chafa failed)")
    return 0
  fi
  written_light=$((written_light + 1))
}

render() {
  local key="$1" href="$2" white="$3" png cached lum
  if [ -z "$href" ]; then
    skipped+=("$key (no full/default logo in the payload)")
    return 0
  fi
  if ! cached=$(fetch "$key" "$href"); then
    skipped+=("$key (fetch failed: $href)")
    return 0
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
  local stem
  stem=$(file_stem "$key")
  mkdir -p "assets/logos/${key%/*}"
  if ! chafa --size="$SIZE" --symbols="$SYMBOLS" -c full -w 9 -f symbols --bg black "$png" \
       > "assets/logos/$stem.ans"; then
    rm -f "assets/logos/$stem.ans"
    skipped+=("$key (chafa failed)")
    return 0
  fi
  written=$((written + 1))
  echo "wrote assets/logos/$stem.ans ($(wc -l < "assets/logos/$stem.ans" | tr -d ' ') rows)"
  render_light "$key" "$cached" "$white"
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
  while IFS=$'\t' read -r key href white; do
    [ -n "$key" ] || continue
    # ncaa is one bucket for CFB and CBB: a school ranked in both polls
    # carries one id and one mark. Render it once.
    grep -qxF "$key" "$seen" && continue
    echo "$key" >> "$seen"
    render "$key" "$href" "${white:-}"
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
    key=$(stem_key "${key%.ans}")
    echo "    (\"$key\", include_str!(\"../../$f\")),"
  done
  echo "];"
  echo
  echo "/// The same marks composited for a light ground (v3.4 §7): keyed"
  echo "/// identically, selected by [\`super::hero_mark\`] when the active"
  echo "/// theme's ground is a light one."
  echo "pub(super) const LIGHT_LOGO_SOURCES: &[(&str, &str)] = &["
  find assets/logos-light -name '*.ans' | sort | while read -r f; do
    key=${f#assets/logos-light/}
    key=$(stem_key "${key%.ans}")
    echo "    (\"$key\", include_str!(\"../../$f\")),"
  done
  echo "];"
} > "$out"
echo "wrote $out ($(grep -c include_str "$out") marks over both sets)"

echo
echo "rendered $written dark mark(s), $written_light light; ${#skipped[@]} skipped"
for s in ${skipped+"${skipped[@]}"}; do echo "  SKIP $s"; done
echo
echo "light set: ${#subs[@]} contrast note(s) (< $READABLE_SHARE of the mark clears 3:1 on $PAPER)"
for s in ${subs+"${subs[@]}"}; do echo "  $s"; done
echo "still short of the floor after the lift: ${#stubborn[@]}"
for s in ${stubborn+"${stubborn[@]}"}; do echo "  $s"; done
