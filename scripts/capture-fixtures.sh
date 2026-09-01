#!/usr/bin/env bash
# Captures one scoreboard + one summary per league into fixtures/. Off-season
# leagues use a known past date so linescores/plays exist. Re-run deliberately;
# fixtures are frozen inputs, not live mirrors. Summaries are trimmed to the
# keys the mappers read (plays capped at 80) so the tree stays small.
#
# Rows are "league espn-path date" — a plain list, not an associative array,
# because macOS ships bash 3.2 and `declare -A` is a bash 4 feature.
set -euo pipefail
cd "$(dirname "$0")/.."
UA="gameday/fixtures (+https://github.com/WallyMagill/game-day)"
B="https://site.web.api.espn.com/apis/site/v2/sports"
LEAGUES="
nfl  football/nfl                            20260104
cfb  football/college-football               20260829
cbb  basketball/mens-college-basketball      20260307
nba  basketball/nba                          20260415
wnba basketball/wnba                         20260830
nhl  hockey/nhl                              20260412
mlb  baseball/mlb                            20260831
epl  soccer/eng.1                            20260830
mls  soccer/usa.1                            20260830
"
echo "$LEAGUES" | while read -r l path date; do
  [ -z "$l" ] && continue
  q="dates=$date"
  [ "$l" = cfb ] && q="$q&groups=80&limit=300"
  curl -sf -A "$UA" "$B/$path/scoreboard?$q" > "fixtures/${l}_scoreboard_full.json"
  id=$(python3 -c 'import json,sys; e=[x for x in json.load(sys.stdin)["events"] if x["status"]["type"]["state"]=="post"]; print(e[0]["id"] if e else "")' < "fixtures/${l}_scoreboard_full.json")
  if [ -n "$id" ]; then
    curl -sf -A "$UA" "$B/$path/summary?event=$id" \
      | python3 -c 'import json,sys; v=json.load(sys.stdin); k={x:v[x] for x in ("header","scoringPlays","drives","keyEvents","boxscore","leaders") if x in v}; k["plays"]=v.get("plays",[])[:80]; json.dump(k,sys.stdout)' > "fixtures/${l}_summary_full.json"
  else
    echo "!! $l $date: no final (post) event — move the date and re-run" >&2
  fi
  sleep 1
done
ls -la fixtures/*_full.json
