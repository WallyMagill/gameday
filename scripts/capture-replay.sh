#!/usr/bin/env bash
# Capture consecutive scoreboard polls for replay tests.
#
#   scripts/capture-replay.sh <league> <minutes> [event_id]
#
# Every 15 s (the live cadence) the scoreboard is fetched and written as
# fixtures/replay/<league>-<UTC yyyymmdd-hhmm>/NN.json — the full payload,
# untouched, or filtered to `events[] | select(.id == event_id)` so a
# sequence stays small. When the loop ends, the summary for each live event
# in the last payload (or the given one) is written beside them as
# summary-<id>.json. The script prints which consecutive pairs carry a score
# delta; a sequence with no delta is not worth committing — delete it.
#
# A poll that fails is skipped, not fatal: a window is minutes of a real game
# that cannot be re-run, so one 503 must not throw the other 79 away. The
# missing number is simply absent from the sequence (the replay harness reads
# the files it finds, in name order), and nothing half-written is ever left
# behind — each poll lands on a .tmp file that is renamed only on success.
#
# Plain lists, no bash-4 features: macOS ships bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."
league="${1:?league slug (nfl cfb cbb nba wnba nhl mlb epl mls)}"
minutes="${2:?minutes to run}"
event="${3:-}"
UA="gameday/replay-capture (+https://github.com/WallyMagill/gameday)"
B="https://site.web.api.espn.com/apis/site/v2/sports"
case "$league" in
  nfl)  path=football/nfl ;;
  cfb)  path="football/college-football"; extra="&groups=80&limit=300" ;;
  cbb)  path=basketball/mens-college-basketball ;;
  nba)  path=basketball/nba ;;
  wnba) path=basketball/wnba ;;
  nhl)  path=hockey/nhl ;;
  mlb)  path=baseball/mlb ;;
  epl)  path=soccer/eng.1 ;;
  mls)  path=soccer/usa.1 ;;
  *) echo "unknown league $league" >&2; exit 2 ;;
esac
extra="${extra:-}"
stamp=$(date -u +%Y%m%d-%H%M)
dir="fixtures/replay/${league}-${stamp}"
mkdir -p "$dir"
polls=$(( minutes * 60 / 15 ))
echo "capturing $polls polls into $dir"
i=0
while [ "$i" -lt "$polls" ]; do
  n=$(printf '%02d' "$i")
  if [ -n "$event" ]; then
    if ! curl -sf -A "$UA" "$B/$path/scoreboard?limit=300$extra" | jq --arg id "$event" '{events: [.events[] | select(.id == $id)]}' > "$dir/$n.json.tmp"; then
      echo "poll $n failed; continuing" >&2
      rm -f "$dir/$n.json.tmp"
    else
      mv "$dir/$n.json.tmp" "$dir/$n.json"
    fi
  else
    if ! curl -sf -A "$UA" "$B/$path/scoreboard?limit=300$extra" | jq '.' > "$dir/$n.json.tmp"; then
      echo "poll $n failed; continuing" >&2
      rm -f "$dir/$n.json.tmp"
    else
      mv "$dir/$n.json.tmp" "$dir/$n.json"
    fi
  fi
  i=$(( i + 1 ))
  [ "$i" -lt "$polls" ] && sleep 15
done
last="$dir/$(printf '%02d' $(( polls - 1 ))).json"
if [ -n "$event" ]; then ids="$event"; else ids=$(jq -r '.events[] | select(.status.type.state=="in") | .id' "$last"); fi
for id in $ids; do
  curl -sf -A "$UA" "$B/$path/summary?event=$id" | jq '.' > "$dir/summary-$id.json"
  sleep 1
done
# Which consecutive pairs carry a score delta, per event.
prev=""
for f in "$dir"/[0-9][0-9].json; do
  if [ -n "$prev" ]; then
    jq -n --slurpfile a "$prev" --slurpfile b "$f" '
      [ $a[0].events[] as $e | ($b[0].events[] | select(.id == $e.id)) as $n
        | ($e.competitions[0].competitors | map(.score)) as $s0
        | ($n.competitions[0].competitors | map(.score)) as $s1
        | select($s0 != $s1) | "\($e.id) \($s0) -> \($s1)" ] | .[]' -r | sed "s|^|$(basename "$prev") -> $(basename "$f"): |"
  fi
  prev="$f"
done
echo "done: $dir"
