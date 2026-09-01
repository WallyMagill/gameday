use crate::domain::*;
use serde_json::Value;
use time::UtcOffset;

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing field {0}")]
    Missing(&'static str),
    /// One event in a scoreboard couldn't be mapped. Carries the event id so
    /// the skip line names which row was dropped and which path was missing.
    #[error("event {id}: missing {path}")]
    Event { id: String, path: &'static str },
}

fn hex_color(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(180);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(180);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(180);
        [r, g, b]
    } else {
        [180, 180, 180]
    }
}

fn status_from(state: &str) -> Status {
    match state {
        "in" => Status::Live,
        "post" => Status::Final,
        _ => Status::Pre,
    }
}

fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "TH",
        (1, _) => "ST",
        (2, _) => "ND",
        (3, _) => "RD",
        _ => "TH",
    };
    format!("{n}{suffix}")
}

/// Human period/inning/minute label per league. Prefers ESPN's own
/// `status.type.shortDetail` where it carries the label ("Bot 7th"), and
/// falls back to the period number otherwise.
fn period_label(
    league: League,
    status: Status,
    period: i64,
    short_detail: &str,
    display_clock: &str,
) -> String {
    if status == Status::Pre {
        return String::new();
    }
    match league {
        League::Nfl | League::Cfb | League::Nba | League::Wnba => match period {
            0 => String::new(),
            1..=4 => format!("Q{period}"),
            _ => "OT".into(),
        },
        League::Cbb => match period {
            0 => String::new(),
            1 => "1ST HALF".into(),
            2 => "2ND HALF".into(),
            _ => "OT".into(),
        },
        League::Nhl => match period {
            0 => String::new(),
            1..=3 => ordinal(period),
            4 => "OT".into(),
            _ => "SO".into(),
        },
        League::Mlb => {
            // Live shortDetail is "Top 9th" / "Bot 7th" / "Mid 9th"; post is "Final".
            // Pre would be a date string, but Pre returns early above.
            if !short_detail.is_empty() {
                short_detail.to_uppercase()
            } else if period > 0 {
                ordinal(period)
            } else {
                String::new()
            }
        }
        League::Epl | League::Mls => {
            // Soccer shows the match minute ("63'", "90'+3'"); ESPN keeps it
            // in displayClock. Post games show FT/AET via shortDetail.
            if status == Status::Final {
                if short_detail.is_empty() { "FT".into() } else { short_detail.to_uppercase() }
            } else {
                display_clock.trim().to_string()
            }
        }
    }
}

/// `rank` is the competitor's `curatedRank` (AP/coaches poll), which sits on
/// the competitor, not on `team`.
fn team_from(league: League, v: &Value, rank: &Value) -> Option<Team> {
    let id = v.get("id")?.as_str()?.to_string();
    let abbr = v.get("abbreviation")?.as_str()?.to_string();
    Some(Team {
        id,
        logo_key: format!("{}/{}", league.slug(), abbr.to_lowercase()),
        name: v.get("name")
            .or_else(|| v.get("shortDisplayName"))
            .or_else(|| v.get("displayName"))
            .and_then(|x| x.as_str())
            .unwrap_or(&abbr)
            .to_string(),
        location: v.get("location").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        record: String::new(),
        color: hex_color(v.get("color").and_then(|x| x.as_str()).unwrap_or("")),
        alt_color: hex_color(v.get("alternateColor").and_then(|x| x.as_str()).unwrap_or("")),
        abbr,
        // ESPN sends 99 for "unranked"; only 1..=25 is a real poll rank.
        rank: rank["current"].as_u64().filter(|&n| (1..=25).contains(&n)).map(|n| n as u8),
    })
}

fn record_from(competitor: &Value) -> String {
    // Real payloads carry [{type:"total"},{type:"home"},{type:"road"}]; pick
    // the overall record explicitly, not positionally. CBB preseason can send
    // `records: null` — the empty-string fallback covers it.
    let records = competitor["records"].as_array();
    records
        .and_then(|rs| rs.iter().find(|r| r["type"].as_str() == Some("total")))
        .or_else(|| records.and_then(|rs| rs.first()))
        .and_then(|r| r["summary"].as_str())
        .unwrap_or("")
        .to_string()
}

/// Football red-zone meter from `competition.situation`. ESPN sends an
/// `isRedZone` bool on live feeds; older/synthetic payloads may lack it, so
/// fall back to parsing `possessionText` ("TB 3" = ball on TB's 3-yard line):
/// red zone when the possessing team is inside the OPPONENT'S 20.
fn redzone_from(sit: &Value, possession_abbr: Option<&str>) -> Option<Meter> {
    let text = sit["possessionText"].as_str()?;
    let (territory, yards) = text.rsplit_once(' ')?;
    let yards: u8 = yards.parse().ok()?;
    let in_opponent_territory = possession_abbr.is_some_and(|p| !territory.eq_ignore_ascii_case(p));
    let in_red_zone = sit["isRedZone"]
        .as_bool()
        .unwrap_or(in_opponent_territory && yards <= 20);
    if in_red_zone && yards <= 20 {
        Some(Meter::RedZone { yards_to_goal: yards })
    } else {
        None
    }
}

/// Live-game meter per league. `None` when the sport has no meter (soccer),
/// when the game isn't live, or when the data to build one isn't in the feed.
fn meter_from(
    league: League,
    status: Status,
    sit: &Value,
    possession_abbr: Option<&str>,
    home_score: u16,
    away_score: u16,
) -> Option<Meter> {
    if status != Status::Live {
        return None;
    }
    match league {
        League::Nfl | League::Cfb => redzone_from(sit, possession_abbr),
        League::Nba | League::Wnba | League::Cbb => Some(Meter::Lead {
            plus_minus: home_score as i16 - away_score as i16,
        }),
        League::Mlb => Some(Meter::Diamond {
            occupied: [
                sit["onFirst"].as_bool().unwrap_or(false),
                sit["onSecond"].as_bool().unwrap_or(false),
                sit["onThird"].as_bool().unwrap_or(false),
            ],
        }),
        // NHL penalty clock: no live NHL fixture existed at mapping time
        // (2026-08-29, preseason) to confirm which situation field carries
        // penalty state — left unmapped rather than guessing a field name.
        League::Nhl => None,
        League::Epl | League::Mls => None,
    }
}

/// Display odds from `competitions[].odds[0]`: `details` is ESPN's
/// pre-formatted spread ("CIN -3.5"), `overUnder` the total. Whichever parts
/// exist are joined ("CIN -3.5  O/U 51.5"); None when the array is absent or
/// empty (finals — ESPN drops odds once a game completes).
fn odds_from(odds: &Value) -> Option<String> {
    let first = odds.as_array()?.first()?;
    let details = first["details"].as_str().filter(|s| !s.is_empty());
    let over_under = first["overUnder"].as_f64();
    match (details, over_under) {
        (Some(d), Some(ou)) => Some(format!("{d}  O/U {ou}")),
        (Some(d), None) => Some(d.to_string()),
        (None, Some(ou)) => Some(format!("O/U {ou}")),
        (None, None) => None,
    }
}

pub fn map_scoreboard(league: League, json: &str, offset: UtcOffset) -> Result<Vec<Game>, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let events = v.get("events").and_then(|e| e.as_array()).ok_or(MapError::Missing("events"))?;
    let mut out = Vec::with_capacity(events.len());
    for ev in events {
        match map_event(league, ev, offset) {
            Ok(g) => out.push(g),
            Err(e) => {
                // One placeholder row must not erase the league (spec §2).
                eprintln!("gameday: {} scoreboard: skipped {e}", league.slug());
            }
        }
    }
    // ...but a slate where NOTHING maps is schema drift, not a quiet day. The
    // provider caches a body only after it maps (spec §3), so returning Ok here
    // would let a drifted payload evict the last-good cache. A genuinely empty
    // `events: []` is still Ok — there just are no games.
    if out.is_empty() && !events.is_empty() {
        eprintln!(
            "gameday: {} scoreboard: {} events, none mappable",
            league.slug(),
            events.len()
        );
        return Err(MapError::Missing("events[*] (no event mapped)"));
    }
    Ok(out)
}

/// One `events[]` entry -> `Game`. Fallible per event so a malformed row is a
/// skipped tile, not a dead league.
pub fn map_event(league: League, ev: &Value, offset: UtcOffset) -> Result<Game, MapError> {
    let id = ev.get("id").and_then(|x| x.as_str()).ok_or(MapError::Missing("id"))?.to_string();
    let miss = |path: &'static str| MapError::Event { id: id.clone(), path };
    let start = ev.get("date").and_then(|x| x.as_str()).and_then(|s| crate::text::local_time(s, offset));
    let comp = ev
        .get("competitions")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| miss("competitions[0]"))?;
    let st = &comp["status"];
    let status = status_from(st["type"]["state"].as_str().unwrap_or("pre"));
    let display_clock = st["displayClock"].as_str().unwrap_or("");
    let period = period_label(
        league,
        status,
        st["period"].as_i64().unwrap_or(0),
        st["type"]["shortDetail"].as_str().unwrap_or(""),
        display_clock,
    );
    // Baseball has no game clock and soccer's minute already lives in the
    // period label; a raw "0:00" next to them is noise. A final's
    // displayClock is whatever ESPN left behind (a WNBA final probed
    // 2026-08-30 carried "10:00" — the fixture keeps one), so it's dropped
    // too: nothing is on the clock once the game is over.
    let clock = match league {
        League::Mlb | League::Epl | League::Mls => String::new(),
        _ if status == Status::Final => String::new(),
        _ => display_clock.to_string(),
    };
    let comps = comp
        .get("competitors")
        .and_then(|c| c.as_array())
        .ok_or_else(|| miss("competitors"))?;
    let mut home = None;
    let mut away = None;
    let mut home_score = 0u16;
    let mut away_score = 0u16;
    let mut linescore_away: Vec<u16> = vec![];
    let mut linescore_home: Vec<u16> = vec![];
    let mut hits: (Option<u16>, Option<u16>) = (None, None);
    let mut errors: (Option<u16>, Option<u16>) = (None, None);
    for c in comps {
        let mut team =
            team_from(league, &c["team"], &c["curatedRank"]).ok_or_else(|| miss("competitors[].team"))?;
        team.record = record_from(c);
        // NHL shots on goal: skipped — no NHL fixture exists and the live
        // scoreboard (2026-08-29, all preseason `pre`) had competitors
        // with `statistics: []`, so the field name couldn't be verified.
        let score = c["score"].as_str().unwrap_or("0").parse().unwrap_or(0);
        let ls: Vec<u16> = c["linescores"]
            .as_array()
            .map(|a| a.iter().map(|p| p["value"].as_f64().unwrap_or(0.0) as u16).collect())
            .unwrap_or_default();
        let h = c["hits"].as_u64().map(|n| n.min(u16::MAX as u64) as u16);
        let e = c["errors"].as_u64().map(|n| n.min(u16::MAX as u64) as u16);
        match c["homeAway"].as_str() {
            Some("home") => {
                home_score = score;
                home = Some(team);
                linescore_home = ls;
                hits.1 = h;
                errors.1 = e;
            }
            _ => {
                away_score = score;
                away = Some(team);
                linescore_away = ls;
                hits.0 = h;
                errors.0 = e;
            }
        }
    }
    let home = home.ok_or_else(|| miss("competitors[homeAway=home]"))?;
    let away = away.ok_or_else(|| miss("competitors[homeAway=away]"))?;
    // Pair only the periods both sides have played: a bottom half that hasn't
    // happened yet is not a zero.
    let n = linescore_away.len().min(linescore_home.len());
    let linescore: Vec<(u16, u16)> = (0..n).map(|i| (linescore_away[i], linescore_home[i])).collect();
    let sit_v = &comp["situation"];
    let abbr_for_id = |tid: Option<&str>| -> Option<String> {
        tid.and_then(|tid| {
            if home.id == tid { Some(home.abbr.clone()) }
            else if away.id == tid { Some(away.abbr.clone()) }
            else { None }
        })
    };
    let situation = if sit_v.is_object() {
        let possession = abbr_for_id(sit_v["possession"].as_str());
        let mut sit = Situation {
            down_distance: sit_v["downDistanceText"].as_str().unwrap_or("").to_string(),
            possession,
            ball_on: sit_v["possessionText"].as_str().map(|s| s.to_string()),
            ..Default::default()
        };
        if league == League::Mlb {
            sit.balls = sit_v["balls"].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
            sit.strikes = sit_v["strikes"].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
            sit.outs = sit_v["outs"].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
            sit.on_base = Some([
                sit_v["onFirst"].as_bool().unwrap_or(false),
                sit_v["onSecond"].as_bool().unwrap_or(false),
                sit_v["onThird"].as_bool().unwrap_or(false),
            ]);
            sit.pitcher = sit_v["pitcher"]["athlete"]["shortName"].as_str().map(str::to_string);
            sit.batter = sit_v["batter"]["athlete"]["shortName"].as_str().map(str::to_string);
            sit.due_up = sit_v["dueUp"]
                .as_array()
                .map(|a| a.iter().filter_map(due_up_line).collect())
                .unwrap_or_default();
            // Compose the headline: "2 OUT · 1-2".
            if let Some(headline) = sit.mlb_count_headline() {
                sit.down_distance = headline;
            }
        }
        // shot_clock stays None: no shot-clock field exists under
        // situation/status in the wnba fixture or the live NBA/WNBA
        // scoreboards (checked 2026-08-29). The tile chip renders only
        // when a value is present, so real data simply shows no chip.
        Some(sit)
    } else {
        None
    };
    // Timeouts ride on `situation`, so they're live-only — pre/post games
    // carry none and the tile shows no pips.
    let timeouts = match (sit_v["awayTimeouts"].as_u64(), sit_v["homeTimeouts"].as_u64()) {
        (Some(a), Some(h)) => Some((a.min(9) as u8, h.min(9) as u8)),
        _ => None,
    };
    let mut last_plays = Vec::new();
    if let Some(text) = sit_v["lastPlay"]["text"].as_str() {
        // Attribute to the team ESPN credits on the play; fall back to
        // the possessing team when the play carries no team.
        let team = abbr_for_id(sit_v["lastPlay"]["team"]["id"].as_str())
            .or_else(|| situation.as_ref().and_then(|s| s.possession.clone()))
            .unwrap_or_default();
        // MLB's lastPlay is a pitch row; take the human label plus the batter,
        // and put the inning where the (nonexistent) clock would go.
        let text = if league == League::Mlb {
            mlb_last_play_text(&sit_v["lastPlay"]).unwrap_or_else(|| text.to_string())
        } else {
            text.to_string()
        };
        last_plays.push(Play {
            clock: if league == League::Mlb {
                String::new()
            } else {
                sit_v["lastPlay"]["clock"]["displayValue"].as_str().unwrap_or(&clock).to_string()
            },
            period: if league == League::Mlb { mlb_inning_tag(&period) } else { String::new() },
            team,
            text,
            scoring: false,
        });
    }
    let extras = match league {
        League::Mlb => Extras::Baseball {
            hits: hits.0.zip(hits.1),
            errors: errors.0.zip(errors.1),
        },
        League::Epl | League::Mls => Extras::Soccer {
            events: details_from(&comp["details"], &abbr_for_id),
        },
        // Drive text lives on the summary, not the scoreboard; shots on goal
        // weren't confirmable without a live NHL feed. Both stay None here.
        League::Nfl | League::Cfb => Extras::Football { drive: None },
        League::Nhl => Extras::Hockey { shots: None },
        _ => Extras::None,
    };
    let odds = odds_from(&comp["odds"]);
    let broadcast = comp["broadcasts"].as_array()
        .and_then(|b| b.first())
        .and_then(|b| b["names"].as_array())
        .and_then(|n| n.first())
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let meter = meter_from(
        league,
        status,
        sit_v,
        situation.as_ref().and_then(|s| s.possession.as_deref()),
        home_score,
        away_score,
    );
    Ok(Game {
        id, league, home, away, home_score, away_score, status, period, clock,
        situation, last_plays, meter, start, broadcast, odds,
        scoring_plays: vec![], linescore, timeouts, extras,
    })
}

/// "BOT 7TH" -> "B7", "TOP 9TH" -> "T9", "MID 5TH"/"END 8TH" -> "M5"/"E8".
fn mlb_inning_tag(period: &str) -> String {
    let mut it = period.split_whitespace();
    let (Some(half), Some(num)) = (it.next(), it.next()) else { return String::new() };
    let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
    match half.chars().next() {
        Some(c) => format!("{c}{digits}"),
        None => String::new(),
    }
}

/// A play's own `period` object — `{"type":"Top","number":9}` -> "T9".
/// Empty when the play carries no period (football drives, soccer keyEvents).
fn inning_tag(period: &Value) -> String {
    // First char, not a byte slice: a non-ASCII type would panic on `&t[..1]`.
    match (period["type"].as_str().and_then(|t| t.chars().next()), period["number"].as_u64()) {
        (Some(c), Some(n)) => format!("{}{n}", c.to_uppercase()),
        _ => String::new(),
    }
}

/// `lastPlay.type.alternativeText` ("Walk", "Strikeout") + the batter — the
/// `text` field is the pitch ("Pitch 6 : Ball 3"), which nobody wants.
fn mlb_last_play_text(lp: &Value) -> Option<String> {
    let label = lp["type"]["alternativeText"].as_str().or(lp["type"]["text"].as_str())?;
    let batter = lp["athletesInvolved"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["shortName"].as_str());
    Some(match batter {
        Some(b) => format!("{label} — {b}"),
        None => label.to_string(),
    })
}

/// One `situation.dueUp[]` entry: "A. Riley (2-3, HR)".
fn due_up_line(v: &Value) -> Option<String> {
    let name = v["athlete"]["shortName"].as_str()?;
    Some(match v["summary"].as_str() {
        Some(s) if !s.is_empty() => format!("{name} ({s})"),
        _ => name.to_string(),
    })
}

/// Soccer `competition.details[]`: goals/cards/subs with minute and player.
/// Anything else in the array (VAR reviews, kickoff markers) is dropped.
fn details_from(details: &Value, abbr_for_id: &dyn Fn(Option<&str>) -> Option<String>) -> Vec<MatchEvent> {
    let Some(arr) = details.as_array() else { return vec![] };
    arr.iter()
        .filter_map(|d| {
            let kind = if d["scoringPlay"].as_bool() == Some(true) {
                if d["ownGoal"].as_bool() == Some(true) {
                    EventKind::OwnGoal
                } else if d["penaltyKick"].as_bool() == Some(true) {
                    EventKind::Penalty
                } else {
                    EventKind::Goal
                }
            } else if d["redCard"].as_bool() == Some(true) {
                EventKind::Red
            } else if d["yellowCard"].as_bool() == Some(true) {
                EventKind::Yellow
            } else if d["type"]["text"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("Substitution")) {
                EventKind::Sub
            } else {
                return None;
            };
            Some(MatchEvent {
                minute: d["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                kind,
                team: abbr_for_id(d["team"]["id"].as_str()).unwrap_or_default(),
                player: d["athletesInvolved"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|a| a["shortName"].as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

pub fn map_summary(json: &str) -> Result<Summary, MapError> {
    let v: Value = serde_json::from_str(json)?;
    // Soccer keyEvents and the flat plays arrays credit teams by id only;
    // the summary header carries the id -> abbreviation map.
    let mut abbr_by_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(comps) = v["header"]["competitions"][0]["competitors"].as_array() {
        for c in comps {
            if let (Some(id), Some(abbr)) =
                (c["team"]["id"].as_str(), c["team"]["abbreviation"].as_str())
            {
                abbr_by_id.insert(id.to_string(), abbr.to_string());
            }
        }
    }
    let team_of = |p: &Value| -> String {
        p["team"]["abbreviation"]
            .as_str()
            .map(str::to_string)
            .or_else(|| p["team"]["id"].as_str().and_then(|id| abbr_by_id.get(id).cloned()))
            .unwrap_or_default()
    };
    let mut scoring_plays: Vec<Play> = v["scoringPlays"].as_array().cloned().unwrap_or_default()
        .iter()
        .filter_map(|p| {
            Some(Play {
                clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                period: String::new(),
                team: team_of(p),
                text: p["text"].as_str()?.to_string(),
                scoring: true,
            })
        })
        .collect();
    let mut plays = Vec::new();
    // Football: drives.previous carries the play-by-play.
    if let Some(prev) = v["drives"]["previous"].as_array() {
        for d in prev {
            let drive_team = d["team"]["abbreviation"].as_str().unwrap_or("");
            if let Some(ps) = d["plays"].as_array() {
                for p in ps {
                    if let Some(text) = p["text"].as_str() {
                        plays.push(Play {
                            clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                            period: String::new(),
                            team: drive_team.to_string(),
                            text: text.to_string(),
                            scoring: p["scoringPlay"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
        }
    }
    // Soccer: no drives/scoringPlays — goals, cards and subs live under
    // keyEvents (verified against live EPL/MLS summaries 2026-08-29).
    // Baseball/basketball/hockey: a flat `plays` array. Both shapes share
    // text/clock/scoringPlay, so one mapper covers them.
    if plays.is_empty() {
        for source in [&v["keyEvents"], &v["plays"]] {
            let Some(events) = source.as_array() else { continue };
            for p in events {
                let Some(text) = p["text"].as_str().filter(|t| !t.is_empty()) else {
                    continue; // delay/period markers carry no text
                };
                // MLB tags rows: P pitch, N at-bat narrative, S scoring, I
                // inning marker, A batter/pitcher start, C substitution. Only
                // N and S are the feed a fan reads (verified 2026-08-31 on a
                // live MLB summary: 285 P vs 74 N + 11 S).
                if matches!(p["summaryType"].as_str(), Some("P") | Some("I") | Some("A") | Some("C"))
                {
                    continue;
                }
                let scoring = p["scoringPlay"].as_bool().unwrap_or(false);
                // NHL power-play goal: strength id "702" (verified on live NHL
                // goals 2026-08-31). No other NHL strength field is trusted.
                let text = if scoring && p["strength"]["id"].as_str() == Some("702") {
                    format!("PP · {text}")
                } else {
                    text.to_string()
                };
                plays.push(Play {
                    clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                    period: inning_tag(&p["period"]),
                    team: team_of(p),
                    text,
                    scoring,
                });
            }
            if !plays.is_empty() {
                break;
            }
        }
        // No scoringPlays list in these feeds: derive it (newest first) so
        // goals still reach the flash/marking path.
        if scoring_plays.is_empty() {
            scoring_plays = plays.iter().filter(|p| p.scoring).rev().cloned().collect();
        }
    }
    // No truncation here: the full list is newest-first and the display cap
    // belongs to the tile that renders it.
    plays.reverse();
    // Meter stays None here on purpose: the scoreboard mapping owns meters and
    // App::merge_summary never reads a summary meter, so mapping one would be
    // dead data pretending to be live.
    Ok(Summary { last_plays: plays, scoring_plays, meter: None })
}

/// One box-score side's stats, flat (`[{name, displayValue}]`) or grouped
/// (`[{name, stats:[{name, displayValue}]}]`, MLB) — both become
/// (name, label, displayValue) triples.
fn stat_rows(side: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for s in side["statistics"].as_array().cloned().unwrap_or_default() {
        if let Some(group) = s["stats"].as_array() {
            for g in group {
                if let (Some(n), Some(v)) = (g["name"].as_str(), g["displayValue"].as_str()) {
                    out.push((
                        n.to_string(),
                        g["label"].as_str().unwrap_or(n).to_string(),
                        v.to_string(),
                    ));
                }
            }
        } else if let (Some(n), Some(v)) = (s["name"].as_str(), s["displayValue"].as_str()) {
            out.push((
                n.to_string(),
                s["label"].as_str().unwrap_or(n).to_string(),
                v.to_string(),
            ));
        }
    }
    out
}

/// Box score from the same summary payload `map_summary` reads:
/// `boxscore.teams[].statistics[]` (label/displayValue, sides identified by
/// `homeAway`, rows paired by stat `name`) and `leaders[].leaders[]` (each
/// category's top athlete). Verified against a real NFL summary
/// (fixtures/nfl_boxscore.json, event 401873297).
pub fn map_stats(json: &str) -> Result<GameStats, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let teams = v["boxscore"]["teams"]
        .as_array()
        .ok_or(MapError::Missing("boxscore.teams"))?;
    let side = |which: &str| -> Option<&Value> {
        teams.iter().find(|t| t["homeAway"].as_str() == Some(which))
    };
    let away = side("away").ok_or(MapError::Missing("boxscore.teams[homeAway=away]"))?;
    let home = side("home").ok_or(MapError::Missing("boxscore.teams[homeAway=home]"))?;
    let home_stats = stat_rows(home);
    let mut rows = Vec::new();
    for (name, label, away_val) in stat_rows(away) {
        // Pair by stat name, not position — order is a payload accident.
        let Some((_, _, home_val)) = home_stats.iter().find(|(n, _, _)| *n == name) else {
            continue; // one-sided stat: skip rather than render a blank cell
        };
        rows.push(StatRow {
            label,
            away: away_val,
            home: home_val.clone(),
        });
    }
    let mut leaders = Vec::new();
    for team_block in v["leaders"].as_array().cloned().unwrap_or_default() {
        let team = team_block["team"]["abbreviation"]
            .as_str()
            .unwrap_or("")
            .to_string();
        for cat in team_block["leaders"].as_array().cloned().unwrap_or_default() {
            let Some(top) = cat["leaders"].get(0) else { continue };
            let Some(value) = top["displayValue"].as_str() else { continue };
            let athlete = top["athlete"]["shortName"].as_str().unwrap_or("");
            leaders.push(Leader {
                team: team.clone(),
                label: cat["displayName"]
                    .as_str()
                    .or(cat["name"].as_str())
                    .unwrap_or("")
                    .to_string(),
                text: format!("{athlete} {value}").trim().to_string(),
            });
        }
    }
    Ok(GameStats { rows, leaders })
}

/// One standings entry -> row. Wins/losses come from the `stats[]` entries
/// with `type: "wins"`/`"losses"` (numeric `value`); the third column is
/// `"ties"` (football, label "T") or `"otlosses"` (hockey, label "OTL") —
/// both type names verified against the real NFL and NHL payloads 2026-08-30.
fn standing_row_from(entry: &Value) -> Option<StandingRow> {
    let team = &entry["team"];
    let abbr = team["abbreviation"].as_str()?.to_string();
    let stats = entry["stats"].as_array()?;
    let stat = |ty: &str| -> Option<u32> {
        stats
            .iter()
            .find(|s| s["type"].as_str() == Some(ty))
            .and_then(|s| s["value"].as_f64())
            .map(|v| v as u32)
    };
    let (third, third_label) = match stat("ties") {
        Some(t) => (Some(t), "T"),
        None => match stat("otlosses") {
            Some(otl) => (Some(otl), "OTL"),
            None => (None, ""),
        },
    };
    Some(StandingRow {
        name: team["name"]
            .as_str()
            .or_else(|| team["displayName"].as_str())
            .unwrap_or(&abbr)
            .to_string(),
        abbr,
        wins: stat("wins")?,
        losses: stat("losses")?,
        third,
        third_label,
    })
}

/// Standings from `…/apis/v2/sports/{sport}/{slug}/standings` — that path
/// worked directly (NFL + NHL, checked 2026-08-30); the plan's
/// `apis/site/v2` fallback was never needed. Shape: `children[]` (one per
/// conference) each carrying `standings.entries[]`; a league that sends no
/// children gets its root `standings` mapped as a single group.
pub fn map_standings(league: League, json: &str) -> Result<StandingsTable, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let group_from = |name: &Value, standings: &Value| -> Option<StandingsGroup> {
        let rows: Vec<StandingRow> = standings["entries"]
            .as_array()?
            .iter()
            .filter_map(standing_row_from)
            .collect();
        if rows.is_empty() {
            return None; // a group with no mappable rows is noise, not data
        }
        Some(StandingsGroup {
            name: name.as_str().unwrap_or("").to_string(),
            rows,
        })
    };
    let mut groups = Vec::new();
    for c in v["children"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        groups.extend(group_from(&c["name"], &c["standings"]));
    }
    if groups.is_empty() {
        groups.extend(group_from(&v["name"], &v["standings"]));
    }
    if groups.is_empty() {
        return Err(MapError::Missing("children[].standings.entries"));
    }
    Ok(StandingsTable { league, groups })
}
