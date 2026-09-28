#!/usr/bin/env bash
# ESPN drift canary. Runs the real binary's `--once --json` against every
# league with a fresh config dir, then checks what ESPN actually sent:
#
#   scripts/espn-canary.sh [path/to/gameday] [out-dir]
#
#   1. every league fetched — exit 0, nothing on stderr, `stale: false`, and a
#      raw `cache/<league>-scoreboard` for each of the nine (the provider
#      caches a body only after it maps);
#   2. every event mapped — no `skipped` / `none mappable` line in gameday.log;
#   3. every status ESPN sent is one we have looked at — each
#      `competitions[0].status.type.name` is in KNOWN_STATUS;
#   4. gameday agrees with ESPN's structure on every event, and drops none:
#      in → live, pre → pre, post + completed → final, post + !completed → off.
#
# Check 3 is a tripwire, not a spec. gameday never reads the name (it maps
# state + completed); the list exists so the first sighting of a new status
# gets a human look. On a failure: `gameday --once --league <l>` while the game
# is on the board, fix the mapping if it draws wrong, then add the name here.
#
# Leaves once.json, stderr.txt, gameday.log and the raw scoreboards in
# out-dir for the CI artifact. bash 3.2 + jq + awk (macOS runs it too).
set -euo pipefail

# Every status.type.name in a captured fixture (fixtures/, 2026-08/09) plus
# STATUS_CANCELED (fixtures/live/mlb_scoreboard_canceled.json, 2026-09-27).
KNOWN_STATUS="STATUS_SCHEDULED STATUS_IN_PROGRESS STATUS_HALFTIME STATUS_END_PERIOD STATUS_DELAYED STATUS_FINAL STATUS_FULL_TIME STATUS_CANCELED"
LEAGUES="nfl cfb cbb nba wnba nhl mlb epl mls"

bin=${1:-target/release/gameday}
out=${2:-$(mktemp -d)}
mkdir -p "$out"
fails=0
fail() {
    echo "FAIL: $*"
    fails=$((fails + 1))
}

set +e
"$bin" --config-dir "$out" --once --json >"$out/once.json" 2>"$out/stderr.txt"
code=$?
set -e

# 1. every league fetched
[ "$code" -eq 0 ] || fail "gameday --once exited $code, expected 0"
if [ -s "$out/stderr.txt" ]; then
    fail "gameday --once wrote to stderr, expected nothing (a league's fetch failed): $(head -c 400 "$out/stderr.txt")"
fi
stale=$(jq -r '.stale' "$out/once.json" 2>/dev/null || echo unparseable)
[ "$stale" = false ] || fail "once.json stale=$stale, expected false"
for l in $LEAGUES; do
    [ -f "$out/cache/$l-scoreboard" ] || fail "no raw scoreboard for $l in $out/cache, expected one per league in LEAGUES"
done

# 2. every event mapped
if [ -f "$out/gameday.log" ] && grep -E 'skipped|none mappable' "$out/gameday.log" >/dev/null; then
    fail "the mapper dropped events, expected none: $(grep -E 'skipped|none mappable' "$out/gameday.log" | head -5)"
fi

# 3 + 4. one row per raw event, joined against the board gameday printed
: >"$out/raw.tsv"
for f in "$out"/cache/*-scoreboard; do
    [ -f "$f" ] || continue
    l=$(basename "$f" -scoreboard)
    jq -r --arg l "$l" '.events[] | .competitions[0].status.type as $t
        | [$l, .id, ($t.name // "none"), ($t.state // "none"), ($t.completed | tostring)] | @tsv' "$f" >>"$out/raw.tsv"
done
jq -r '.games[] | [.league, .id, .status] | @tsv' "$out/once.json" >"$out/board.tsv" 2>/dev/null || : >"$out/board.tsv"

awk -F'\t' -v known="$KNOWN_STATUS" '
    BEGIN { n = split(known, k, " "); for (i = 1; i <= n; i++) ok[k[i]] = 1 }
    FILENAME == ARGV[1] { board[$1 SUBSEP $2] = $3; next }
    {
        league = $1; id = $2; name = $3; state = $4; done = $5
        if (!(name in ok))
            printf "FAIL: %s %s status %s (state %s, completed %s) is not in KNOWN_STATUS, expected a name gameday has been checked against\n", league, id, name, state, done
        if (state == "in") want = "live"
        else if (state == "pre") want = "pre"
        else if (state == "post" && done == "false") want = "off"
        else if (state == "post") want = "final"
        else { printf "FAIL: %s %s state %s, expected pre|in|post\n", league, id, state; next }
        key = league SUBSEP id
        if (!(key in board))
            printf "FAIL: %s %s (%s) is on ESPN'\''s scoreboard but not on the board, expected every mapped event in once.json\n", league, id, name
        else if (board[key] != want)
            printf "FAIL: %s %s (%s, state %s, completed %s) is %s on the board, expected %s\n", league, id, name, state, done, board[key], want
    }
' "$out/board.tsv" "$out/raw.tsv" >"$out/checks.txt"

grep '^FAIL' "$out/checks.txt" || true
fails=$((fails + $(grep -c '^FAIL' "$out/checks.txt" || true)))
echo "--- $(wc -l <"$out/raw.tsv" | tr -d ' ') events across $(find "$out/cache" -name '*-scoreboard' 2>/dev/null | wc -l | tr -d ' ') leagues"
awk -F'\t' '{ print $1, $3 }' "$out/raw.tsv" | sort | uniq -c | sed 's/^ */  /'

if [ "$fails" -gt 0 ]; then
    echo "espn-canary: $fails problem(s) — raw payloads and gameday output are in $out"
    exit 1
fi
echo "espn-canary: ok"
