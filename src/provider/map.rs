use crate::domain::*;
use crate::provider::kinds;
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

/// Human period/inning/minute label per league, for the game header. Prefers
/// ESPN's own `status.type.shortDetail` where it carries the label ("Bot
/// 7th"), and falls back to the period number otherwise. Distinct from
/// `period_label` below, which tags one play from its own `period` object.
fn game_period_label(
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
                if short_detail.is_empty() {
                    "FT".into()
                } else {
                    short_detail.to_uppercase()
                }
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
    let logo_key = crate::domain::logo_key(league, &id, &abbr);
    Some(Team {
        id,
        logo_key,
        name: v
            .get("name")
            .or_else(|| v.get("shortDisplayName"))
            .or_else(|| v.get("displayName"))
            .and_then(|x| x.as_str())
            .unwrap_or(&abbr)
            .to_string(),
        location: v
            .get("location")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        record: String::new(),
        color: hex_color(v.get("color").and_then(|x| x.as_str()).unwrap_or("")),
        alt_color: hex_color(
            v.get("alternateColor")
                .and_then(|x| x.as_str())
                .unwrap_or(""),
        ),
        abbr,
        // ESPN sends 99 for "unranked"; only 1..=25 is a real poll rank.
        rank: rank["current"]
            .as_u64()
            .filter(|&n| (1..=25).contains(&n))
            .map(|n| n as u8),
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

/// Yards the possessing team has left to the goal it's attacking, from
/// ESPN's absolute `situation.yardLine` (0 = home goal line, 100 = away
/// goal line — see [`Situation::yard_line`]). The home team attacks 100, the
/// away team attacks 0.
fn yards_to_goal(yard_line: u8, possession_is_home: bool) -> u8 {
    let yl = yard_line.min(100);
    if possession_is_home {
        100 - yl
    } else {
        yl
    }
}

/// Live-game meter per league. `None` when the sport has no meter (soccer),
/// when the game isn't live, or when the data to build one isn't in the feed.
/// `red_zone_yards` is football's, precomputed by the caller from
/// `situation.isRedZone` + `situation.yardLine` — there is no
/// text fallback: a feed that doesn't say "red zone" doesn't get one.
fn meter_from(
    league: League,
    status: Status,
    sit: &Value,
    red_zone_yards: Option<u8>,
    home_score: u16,
    away_score: u16,
) -> Option<Meter> {
    if status != Status::Live {
        return None;
    }
    match league {
        League::Nfl | League::Cfb => {
            red_zone_yards.map(|yards_to_goal| Meter::RedZone { yards_to_goal })
        }
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

pub fn map_scoreboard(
    league: League,
    json: &str,
    offset: UtcOffset,
) -> Result<Vec<Game>, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let events = v
        .get("events")
        .and_then(|e| e.as_array())
        .ok_or(MapError::Missing("events"))?;
    let mut out = Vec::with_capacity(events.len());
    for ev in events {
        match map_event(league, ev, offset) {
            Ok(g) => out.push(g),
            Err(e) => {
                // One placeholder row must not erase the league.
                // Once per (league, event): the same bad row is in every
                // poll, so repeating it would be noise, not news.
                crate::log::note_once(
                    &format!("{}:{e}", league.slug()),
                    &format!("gameday: {} scoreboard: skipped {e}", league.slug()),
                );
            }
        }
    }
    // ...but a slate where NOTHING maps is schema drift, not a quiet day. The
    // provider caches a body only after it maps, so returning Ok here
    // would let a drifted payload evict the last-good cache. A genuinely empty
    // `events: []` is still Ok — there just are no games.
    if out.is_empty() && !events.is_empty() {
        // Not deduped: a whole league going unmappable is a live incident,
        // and each occurrence is a data point about when it started.
        crate::log::note(&format!(
            "gameday: {} scoreboard: {} events, none mappable",
            league.slug(),
            events.len()
        ));
        return Err(MapError::Missing("events[*] (no event mapped)"));
    }
    Ok(out)
}

/// One `events[]` entry -> `Game`. Fallible per event so a malformed row is a
/// skipped tile, not a dead league.
pub fn map_event(league: League, ev: &Value, offset: UtcOffset) -> Result<Game, MapError> {
    let id = ev
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or(MapError::Missing("id"))?
        .to_string();
    let miss = |path: &'static str| MapError::Event {
        id: id.clone(),
        path,
    };
    let start = ev
        .get("date")
        .and_then(|x| x.as_str())
        .and_then(|s| crate::text::local_time(s, offset));
    let comp = ev
        .get("competitions")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| miss("competitions[0]"))?;
    let st = &comp["status"];
    let status = status_from(st["type"]["state"].as_str().unwrap_or("pre"));
    let display_clock = st["displayClock"].as_str().unwrap_or("");
    let period = game_period_label(
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
        let mut team = team_from(league, &c["team"], &c["curatedRank"])
            .ok_or_else(|| miss("competitors[].team"))?;
        team.record = record_from(c);
        // NHL shots on goal: skipped — no NHL fixture exists and the live
        // scoreboard (2026-08-29, all preseason `pre`) had competitors
        // with `statistics: []`, so the field name couldn't be verified.
        let score = c["score"].as_str().unwrap_or("0").parse().unwrap_or(0);
        let ls: Vec<u16> = c["linescores"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| p["value"].as_f64().unwrap_or(0.0) as u16)
                    .collect()
            })
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
            Some("away") => {
                away_score = score;
                away = Some(team);
                linescore_away = ls;
                hits.0 = h;
                errors.0 = e;
            }
            // Anything else is schema drift, not an away team: writing it
            // into `away` would render a two-home-team tile as if it were
            // real. Skip the event and let the rest of the slate stand.
            _ => return Err(miss("competitors[].homeAway")),
        }
    }
    let home = home.ok_or_else(|| miss("competitors[homeAway=home]"))?;
    let away = away.ok_or_else(|| miss("competitors[homeAway=away]"))?;
    // Pair only the periods both sides have played: a bottom half that hasn't
    // happened yet is not a zero.
    let n = linescore_away.len().min(linescore_home.len());
    let linescore: Vec<(u16, u16)> = (0..n)
        .map(|i| (linescore_away[i], linescore_home[i]))
        .collect();
    let sit_v = &comp["situation"];
    let abbr_for_id = |tid: Option<&str>| -> Option<String> {
        tid.and_then(|tid| {
            if home.id == tid {
                Some(home.abbr.clone())
            } else if away.id == tid {
                Some(away.abbr.clone())
            } else {
                None
            }
        })
    };
    let situation = if sit_v.is_object() {
        let possession = abbr_for_id(sit_v["possession"].as_str());
        let u8_at = |key: &str| sit_v[key].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
        let mut sit = Situation {
            down_distance: sit_v["downDistanceText"].as_str().unwrap_or("").to_string(),
            possession,
            ball_on: sit_v["possessionText"].as_str().map(|s| s.to_string()),
            // The numbers as ESPN sends them. A league that
            // doesn't send them leaves them None — nothing here is derived
            // from a string.
            down: u8_at("down"),
            distance: u8_at("distance"),
            yard_line: u8_at("yardLine"),
            is_red_zone: sit_v["isRedZone"].as_bool(),
            drive_desc: sit_v["lastPlay"]["drive"]["description"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            ..Default::default()
        };
        if league == League::Mlb {
            sit.balls = sit_v["balls"].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
            sit.strikes = sit_v["strikes"]
                .as_u64()
                .map(|n| n.min(u8::MAX as u64) as u8);
            sit.outs = sit_v["outs"].as_u64().map(|n| n.min(u8::MAX as u64) as u8);
            sit.on_base = Some([
                sit_v["onFirst"].as_bool().unwrap_or(false),
                sit_v["onSecond"].as_bool().unwrap_or(false),
                sit_v["onThird"].as_bool().unwrap_or(false),
            ]);
            sit.pitcher = sit_v["pitcher"]["athlete"]["shortName"]
                .as_str()
                .map(str::to_string);
            sit.batter = sit_v["batter"]["athlete"]["shortName"]
                .as_str()
                .map(str::to_string);
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
    let timeouts = match (
        sit_v["awayTimeouts"].as_u64(),
        sit_v["homeTimeouts"].as_u64(),
    ) {
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
        let score_value = sit_v["lastPlay"]["scoreValue"]
            .as_u64()
            .map(|v| v.min(u8::MAX as u64) as u8);
        last_plays.push(Play {
            id: sit_v["lastPlay"]["id"].as_str().unwrap_or("").to_string(),
            clock: if league == League::Mlb {
                String::new()
            } else {
                sit_v["lastPlay"]["clock"]["displayValue"]
                    .as_str()
                    .unwrap_or(&clock)
                    .to_string()
            },
            period: if league == League::Mlb {
                mlb_inning_tag(&period)
            } else {
                String::new()
            },
            team,
            text,
            // ESPN's own verdict. The observed scoreboard rows carry
            // `scoringPlay: null` (every live capture in fixtures/live, and
            // every pitch and snap seen in the review's caches), so on real
            // data this is usually false even for the play that scored — which
            // is exactly why a score delta with a non-scoring last play asks
            // the summary instead (`app::merge`).
            scoring: sit_v["lastPlay"]["scoringPlay"].as_bool() == Some(true)
                || score_value.is_some_and(|v| v > 0),
            kind: last_play_kind(league, &sit_v["lastPlay"], score_value),
            score_value,
        });
    }
    let extras = match league {
        League::Mlb => Extras::Baseball {
            hits: hits.0.zip(hits.1),
            errors: errors.0.zip(errors.1),
        },
        League::Epl | League::Mls => {
            let events = details_from(&comp["details"], &abbr_for_id);
            let men = men_from_events(&events, &away.abbr, &home.abbr);
            Extras::Soccer { events, men }
        }
        // Football needs no Extras variant: drive text lives on
        // `Situation::drive_desc`, mapped from the scoreboard above. Shots on
        // goal weren't confirmable without a live NHL feed, so that's the
        // only thing still waiting on a source here.
        _ => Extras::None,
    };
    let odds = odds_from(&comp["odds"]);
    let broadcast = comp["broadcasts"]
        .as_array()
        .and_then(|b| b.first())
        .and_then(|b| b["names"].as_array())
        .and_then(|n| n.first())
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    // Red zone, structurally: ESPN's flag says whether, its absolute
    // yardLine says how far, and which goal is being attacked comes from the
    // possessing team's side. Any of the three missing => no meter.
    let red_zone_yards = situation.as_ref().and_then(|s| {
        if s.is_red_zone != Some(true) {
            return None;
        }
        let possession_is_home = sit_v["possession"].as_str() == Some(home.id.as_str());
        s.yard_line.map(|yl| yards_to_goal(yl, possession_is_home))
    });
    let meter = meter_from(
        league,
        status,
        sit_v,
        red_zone_yards,
        home_score,
        away_score,
    );
    // A final's own story: `shortLinkText`, never
    // `description` — the latter is em-dash wire copy ("— Myles Garrett
    // wanted..."), not display prose. Empty/whitespace-only reads as no
    // headline at all.
    let headline = comp["headlines"][0]["shortLinkText"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Game {
        id,
        league,
        home,
        away,
        home_score,
        away_score,
        status,
        period,
        clock,
        situation,
        last_plays,
        meter,
        start,
        broadcast,
        odds,
        headline,
        scoring_plays: vec![],
        linescore,
        timeouts,
        extras,
    })
}

/// The scoreboard's `situation.lastPlay` through the same per-league kind
/// tables the summary uses — the board's last-play line is
/// the same structure at a different cadence, so it gets the same treatment
/// rather than a hardcoded `Other`. An id the table doesn't know stays
/// `Other`; nothing here guesses from text.
fn last_play_kind(league: League, lp: &Value, score_value: Option<u8>) -> PlayKind {
    let type_id = lp["type"]["id"].as_str().unwrap_or("");
    match league {
        League::Nfl | League::Cfb => {
            kinds::football_kind(type_id, lp["scoringType"]["name"].as_str())
        }
        League::Nba | League::Wnba | League::Cbb => kinds::hoops_kind(
            type_id,
            lp["scoringPlay"].as_bool().unwrap_or(false),
            score_value,
            league == League::Cbb,
        ),
        League::Nhl => kinds::nhl_kind(type_id, lp["type"].get("penaltyMinutes").is_some()),
        // MLB's lastPlay IS the pitch row, so its own type id is the pitch
        // outcome id `mlb_kind` wants — no atBatId join needed at this
        // cadence (the summary's join exists because the pitch and the
        // narrative are separate rows there).
        League::Mlb => kinds::mlb_kind(type_id, score_value),
        League::Epl | League::Mls => kinds::soccer_kind(type_id),
    }
}

/// "BOT 7TH" -> "B7", "TOP 9TH" -> "T9", "MID 5TH"/"END 8TH" -> "M5"/"E8".
fn mlb_inning_tag(period: &str) -> String {
    let mut it = period.split_whitespace();
    let (Some(half), Some(num)) = (it.next(), it.next()) else {
        return String::new();
    };
    let digits: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
    match half.chars().next() {
        Some(c) => format!("{c}{digits}"),
        None => String::new(),
    }
}

/// The compact period tag a play row prints beside its clock, from the
/// play's own `period` object, in the grammar the scoreboard's period
/// labels already use: football and pro hoops `Q1`..`Q4` then `OT`;
/// college hoops `1H`/`2H` then `OT`; hockey `P1`..`P3` then `OT`;
/// baseball `T3`/`B9` (top/bottom + inning, via `inning_tag`); soccer plays
/// carry the minute in their clock and no period, so "".
pub fn period_label(league: League, period: &Value) -> String {
    let Some(n) = period["number"].as_u64() else {
        return String::new();
    };
    match league {
        League::Mlb => inning_tag(period),
        League::Epl | League::Mls => String::new(),
        League::Cbb => match n {
            1 => "1H".into(),
            2 => "2H".into(),
            _ => "OT".into(),
        },
        League::Nhl => match n {
            1..=3 => format!("P{n}"),
            _ => "OT".into(),
        },
        League::Nfl | League::Cfb | League::Nba | League::Wnba => match n {
            1..=4 => format!("Q{n}"),
            _ => "OT".into(),
        },
    }
}

/// A play's own `period` object — `{"type":"Top","number":9}` -> "T9".
/// Empty when the play carries no period (football drives, soccer keyEvents).
fn inning_tag(period: &Value) -> String {
    // First char, not a byte slice: a non-ASCII type would panic on `&t[..1]`.
    match (
        period["type"].as_str().and_then(|t| t.chars().next()),
        period["number"].as_u64(),
    ) {
        (Some(c), Some(n)) => format!("{}{n}", c.to_uppercase()),
        _ => String::new(),
    }
}

/// `lastPlay.type.text` ("Ball", "Strike Looking", "Home Run") + the batter.
/// NOT `type.alternativeText`: on MLB that field is the pitch type's
/// *projected* at-bat outcome ("Walk" on a mere ball, "Strikeout" on a called
/// strike) — a live-observed lie, not what happened. `type.text` is the
/// honest pitch/outcome label and agrees with `alternativeText` on terminal
/// plays anyway. The play-level `lp["text"]` field is the pitch-count chatter
/// ("Pitch 6 : Ball 3"), which nobody wants either.
fn mlb_last_play_text(lp: &Value) -> Option<String> {
    let label = lp["type"]["text"].as_str()?;
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
fn details_from(
    details: &Value,
    abbr_for_id: &dyn Fn(Option<&str>) -> Option<String>,
) -> Vec<MatchEvent> {
    let Some(arr) = details.as_array() else {
        return vec![];
    };
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
            } else if d["type"]["text"]
                .as_str()
                .is_some_and(|t| t.eq_ignore_ascii_case("Substitution"))
            {
                EventKind::Sub
            } else {
                return None;
            };
            Some(MatchEvent {
                minute: d["clock"]["displayValue"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                kind,
                team: abbr_for_id(d["team"]["id"].as_str()).unwrap_or_default(),
                player: d["athletesInvolved"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|a| a["shortName"].as_str())
                    .unwrap_or("")
                    .to_string(),
                athlete_id: d["athletesInvolved"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|a| a["id"].as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Men on the field per side, (away, home), from the mapped match events
/// alone. `None` at eleven a side, which is the overwhelming
/// majority of matches: the board only says something when there is
/// something to say.
///
/// The rule is deliberately defensive:
///
/// > `reds(team) = |{ explicit red cards }  ∪  { athletes with ≥ 2 yellows }|`
///
/// ESPN's second-yellow encoding is UNOBSERVED. The one red card we have a
/// real capture of (`fixtures/live/epl_scoreboard_redcard.json`, João Gomes
/// 40') is a *straight* red — a lone type 93. A second yellow could plausibly
/// arrive as a third 94, as a 93, or as both, and this rule is correct under
/// all three: the yellow-pair clause catches the 94-only spelling, the
/// explicit clause catches the 93-only spelling, and taking the UNION over
/// athlete ids means a feed that sends both does not send a side down to
/// nine. Only a card whose detail credits no athlete falls outside the union
/// (it can only be an explicit red, and it is counted on its own).
///
/// Eleven is the starting count, not a cap on reality: a team can finish
/// with seven. `saturating_sub` keeps a malformed feed from wrapping.
fn men_from_events(events: &[MatchEvent], away_abbr: &str, home_abbr: &str) -> Option<(u8, u8)> {
    let reds = |team: &str| -> u8 {
        if team.is_empty() {
            return 0;
        }
        let mine = || events.iter().filter(|e| e.team == team);
        let mut sent_off: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut anonymous = 0u8;
        for e in mine().filter(|e| e.kind == EventKind::Red) {
            match e.athlete_id.as_deref() {
                Some(id) => {
                    sent_off.insert(id);
                }
                // No id to dedupe on; it can still only be one sending-off.
                None => anonymous = anonymous.saturating_add(1),
            }
        }
        let mut yellows: std::collections::HashMap<&str, u8> = std::collections::HashMap::new();
        for e in mine().filter(|e| e.kind == EventKind::Yellow) {
            if let Some(id) = e.athlete_id.as_deref() {
                *yellows.entry(id).or_default() += 1;
            }
        }
        for (id, n) in yellows {
            if n >= 2 {
                sent_off.insert(id);
            }
        }
        (sent_off.len().min(u8::MAX as usize) as u8).saturating_add(anonymous)
    };
    let (a, h) = (
        11u8.saturating_sub(reds(away_abbr)),
        11u8.saturating_sub(reds(home_abbr)),
    );
    (a < 11 || h < 11).then_some((a, h))
}

pub fn map_summary(league: League, json: &str) -> Result<Summary, MapError> {
    let v: Value = serde_json::from_str(json)?;
    // Soccer keyEvents and the flat plays arrays credit teams by id only;
    // the summary header carries the id -> abbreviation map.
    let mut abbr_by_id: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
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
            .or_else(|| {
                p["team"]["id"]
                    .as_str()
                    .and_then(|id| abbr_by_id.get(id).cloned())
            })
            .unwrap_or_default()
    };
    let mut scoring_plays: Vec<Play> = v["scoringPlays"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            Some(Play {
                id: p["id"].as_str().unwrap_or("").to_string(),
                clock: p["clock"]["displayValue"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                period: period_label(league, &p["period"]),
                team: team_of(p),
                text: p["text"].as_str()?.to_string(),
                scoring: true,
                kind: kinds::football_kind(
                    p["type"]["id"].as_str().unwrap_or(""),
                    p["scoringType"]["name"].as_str(),
                ),
                score_value: p["scoreValue"]
                    .as_u64()
                    .map(|v| v.min(u8::MAX as u64) as u8),
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
                        let type_id = p["type"]["id"].as_str().unwrap_or("");
                        let scoring_type = p["scoringType"]["name"].as_str();
                        plays.push(Play {
                            id: p["id"].as_str().unwrap_or("").to_string(),
                            clock: p["clock"]["displayValue"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            period: period_label(league, &p["period"]),
                            team: drive_team.to_string(),
                            text: text.to_string(),
                            scoring: p["scoringPlay"].as_bool().unwrap_or(false),
                            kind: kinds::football_kind(type_id, scoring_type),
                            score_value: p["scoreValue"]
                                .as_u64()
                                .map(|v| v.min(u8::MAX as u64) as u8),
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
            let Some(events) = source.as_array() else {
                continue;
            };
            // MLB only: pass 1 over the raw P rows builds atBatId -> pitch
            // type id — a flat pitch carries no batted-ball outcome, so the
            // kind lives on this pass and rides the narrative row (type 57
            // Play Result) in pass 2 below, joined by atBatId. Home Run (28)
            // wins over any other pitch type id seen in the same at-bat (a P
            // row's own scoreValue is always 0 on the live feed — the run
            // count lives on the narrative row, read in pass 2 as today).
            let mlb_pitch_type_by_at_bat: std::collections::HashMap<&str, &str> =
                if league == League::Mlb {
                    let mut m = std::collections::HashMap::new();
                    for p in events {
                        if p["summaryType"].as_str() != Some("P") {
                            continue;
                        }
                        let Some(at_bat_id) = p["atBatId"].as_str() else {
                            continue;
                        };
                        let type_id = p["type"]["id"].as_str().unwrap_or("");
                        let entry = m.entry(at_bat_id).or_insert(type_id);
                        if type_id == "28" {
                            *entry = "28";
                        }
                    }
                    m
                } else {
                    std::collections::HashMap::new()
                };
            for p in events {
                let Some(text) = p["text"].as_str().filter(|t| !t.is_empty()) else {
                    continue; // delay/period markers carry no text
                };
                // MLB tags rows: P pitch, N at-bat narrative, S scoring, I
                // inning marker, A batter/pitcher start, C substitution. Only
                // N and S are the feed a fan reads (verified 2026-08-31 on a
                // live MLB summary: 285 P vs 74 N + 11 S).
                // Allow-by-default: only MLB rows carry a `summaryType` at
                // all, so this filter's `None` arm (id absent, or a
                // `summaryType: null` row on non-MLB feeds) intentionally
                // falls through unfiltered — NBA/WNBA/CBB/NHL flat plays and
                // soccer keyEvents never set this field.
                if matches!(
                    p["summaryType"].as_str(),
                    Some("P") | Some("I") | Some("A") | Some("C")
                ) {
                    continue;
                }
                let scoring = p["scoringPlay"].as_bool().unwrap_or(false);
                // Play text is the feed's own words. NHL
                // strength used to be collapsed into a "PP · " prefix here;
                // it is structural now and rides `Extras::Hockey`.
                let text = text.to_string();
                let type_id = p["type"]["id"].as_str().unwrap_or("");
                let score_value = p["scoreValue"]
                    .as_u64()
                    .map(|v| v.min(u8::MAX as u64) as u8);
                let kind = match league {
                    League::Nba | League::Wnba | League::Cbb => {
                        // scoringPlay, not shootingPlay: CBB's endpoint
                        // stamps scoreValue:3 on missed threes too —
                        // shootingPlay alone would tag a miss as a make.
                        kinds::hoops_kind(type_id, scoring, score_value, league == League::Cbb)
                    }
                    League::Nhl => {
                        // No fixture distinguishes a `type.penaltyMinutes`
                        // that's absent from one that's present-but-null, so
                        // this can't be proven either way from what we have;
                        // `.is_some()` treats an explicit null as "has
                        // penalty minutes," which only matters if ESPN ever
                        // sends that shape.
                        let has_penalty_minutes = p["type"].get("penaltyMinutes").is_some();
                        kinds::nhl_kind(type_id, has_penalty_minutes)
                    }
                    League::Epl | League::Mls => kinds::soccer_kind(type_id),
                    // The at-bat join: the pitch type id comes from pass 1
                    // above (joined by atBatId), score_value from this row —
                    // the narrative row is the only one that carries the RBI
                    // count.
                    League::Mlb => {
                        let at_bat_id = p["atBatId"].as_str().unwrap_or("");
                        let pitch_type_id = mlb_pitch_type_by_at_bat
                            .get(at_bat_id)
                            .copied()
                            .unwrap_or("");
                        kinds::mlb_kind(pitch_type_id, score_value)
                    }
                    // Unreachable: football fills `plays` from drives above,
                    // so this branch never runs for NFL/CFB.
                    League::Nfl | League::Cfb => PlayKind::Other,
                };
                plays.push(Play {
                    id: p["id"].as_str().unwrap_or("").to_string(),
                    clock: p["clock"]["displayValue"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    period: period_label(league, &p["period"]),
                    team: team_of(p),
                    text,
                    scoring,
                    kind,
                    score_value,
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
    // NHL. Strength and penalties are read off the raw play
    // rows, not the mapped ones: `Play` carries neither, and the raw rows are
    // in feed order (oldest first), which is what "the most recent play's
    // strength" and "penalties oldest first" both need.
    let extras = match league {
        League::Nhl => hockey_extras(&v, &team_of),
        _ => Extras::None,
    };
    // Meter stays None here on purpose: the scoreboard mapping owns meters,
    // and the one meter this payload could build — NHL's penalty clock — is
    // deliberately NOT put on the shared `Game`. It is derived at the
    // zoom from `Extras::Hockey` instead, so the board and `:tv`, which read
    // `game.meter`, cannot show a state only the zoomed game has data for.
    // See `Extras::penalty_meter`.
    Ok(Summary {
        last_plays: plays,
        scoring_plays,
        meter: None,
        extras,
    })
}

/// `Extras::Hockey` from a summary payload: the strength of the most recent
/// play (every play carries `strength.id`) and every penalty called so far.
/// `Extras::None` when the payload has no plays at all — an empty list is
/// not evidence of even strength.
fn hockey_extras(v: &Value, team_of: &dyn Fn(&Value) -> String) -> Extras {
    let Some(raw) = v["plays"].as_array().filter(|a| !a.is_empty()) else {
        return Extras::None;
    };
    let strength =
        kinds::hockey_strength(raw[raw.len() - 1]["strength"]["id"].as_str().unwrap_or(""));
    let penalties = raw
        .iter()
        .filter_map(|p| {
            // Same signal `nhl_kind` uses for PlayKind::HockeyPenalty: the
            // penalty id space is unenumerable, `type.penaltyMinutes` is the
            // fact. ESPN sends it as a string ("2").
            let minutes: u8 = p["type"]["penaltyMinutes"].as_str()?.parse().ok()?;
            Some(PenaltyEvent {
                team: team_of(p),
                minutes,
                kind: p["type"]["penaltyType"].as_str().unwrap_or("").to_string(),
                period: p["period"]["number"]
                    .as_u64()
                    .unwrap_or(0)
                    .min(u8::MAX as u64) as u8,
                clock: p["clock"]["displayValue"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect();
    Extras::Hockey {
        strength,
        penalties,
    }
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
        for cat in team_block["leaders"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let Some(top) = cat["leaders"].get(0) else {
                continue;
            };
            let Some(value) = top["displayValue"].as_str() else {
                continue;
            };
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
/// with `type: "wins"`/`"losses"` (numeric `value`); the third column is the
/// one the *sport* actually keeps — ties for football and soccer (label "T"),
/// overtime losses for hockey ("OTL"), nothing for everyone else. ESPN sends
/// `ties: 0` for all 30 MLB teams and all 30 NBA teams, so trusting the feed
/// alone prints a column of zeroes for a record baseball hasn't kept since
/// 2016; the league decides instead. Type names verified against the real NFL
/// and NHL payloads 2026-08-30.
fn standing_row_from(league: League, entry: &Value) -> Option<StandingRow> {
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
    let (third, third_label) = match league {
        League::Nfl | League::Cfb | League::Epl | League::Mls => match stat("ties") {
            Some(t) => (Some(t), "T"),
            None => (None, ""),
        },
        League::Nhl => match stat("otlosses") {
            Some(otl) => (Some(otl), "OTL"),
            None => (None, ""),
        },
        _ => (None, ""),
    };
    // The college-football feed omits a stat whose value is zero, so a 1-0
    // team carries `wins` and no `losses` (verified against the live FBS
    // payload 2026-08-31: 138 entries, zero of them with a `losses` stat).
    // Requiring both dropped every undefeated team — and, in preseason, the
    // whole table. One of the two is enough; the missing one is the zero the
    // feed didn't bother to send. An entry with neither is still not a row.
    let (wins, losses) = (stat("wins"), stat("losses"));
    if wins.is_none() && losses.is_none() {
        return None;
    }
    Some(StandingRow {
        name: team["name"]
            .as_str()
            .or_else(|| team["displayName"].as_str())
            .unwrap_or(&abbr)
            .to_string(),
        abbr,
        wins: wins.unwrap_or(0),
        losses: losses.unwrap_or(0),
        third,
        third_label,
    })
}

/// Standings from the URL `espn::standings_url` builds — that path worked
/// directly (NFL + NHL, checked 2026-08-30); the prefixed `site` fallback was
/// never needed. Shape: `children[]` (one per
/// conference) each carrying `standings.entries[]`; a league that sends no
/// children gets its root `standings` mapped as a single group. A child that
/// carries its own `children[]` (divisions under a conference) contributes one
/// group per grandchild, named "conference · division".
///
/// Rows come out of the feed in ESPN's own order, which is not standings
/// order; every group is sorted here — win pct desc, wins desc, name asc — so
/// a table headed STANDINGS actually stands them.
pub fn map_standings(league: League, json: &str) -> Result<StandingsTable, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let group_from = |name: String, standings: &Value| -> Option<StandingsGroup> {
        let mut rows: Vec<StandingRow> = standings["entries"]
            .as_array()?
            .iter()
            .filter_map(|e| standing_row_from(league, e))
            .collect();
        if rows.is_empty() {
            return None; // a group with no mappable rows is noise, not data
        }
        rows.sort_by(|a, b| {
            win_pct(b)
                .total_cmp(&win_pct(a))
                .then(b.wins.cmp(&a.wins))
                .then(a.name.cmp(&b.name))
        });
        Some(StandingsGroup { name, rows })
    };
    let name_of = |v: &Value| v["name"].as_str().unwrap_or("").to_string();
    let mut groups = Vec::new();
    for c in v["children"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        let conference = name_of(c);
        let divisions = c["children"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        if divisions.is_empty() {
            groups.extend(group_from(conference, &c["standings"]));
        } else {
            for d in divisions {
                let name = format!("{conference} · {}", name_of(d));
                groups.extend(group_from(name, &d["standings"]));
            }
        }
    }
    if groups.is_empty() {
        groups.extend(group_from(name_of(&v), &v["standings"]));
    }
    if groups.is_empty() {
        return Err(MapError::Missing("children[].standings.entries"));
    }
    // The season label the feed prints on itself ("2025-26"): NBA/NHL/CBB
    // serve last season's table all summer, and an unlabeled one reads as
    // today's.
    let season = v["season"]["displayName"]
        .as_str()
        .or_else(|| v["seasonDisplayName"].as_str())
        .map(str::to_string);
    Ok(StandingsTable {
        league,
        season,
        groups,
        fetched_at: None,
    })
}

/// Winning percentage, the sort key: the third column is worth half a win and
/// a full game played, for every league that keeps one. For the NHL that is
/// points percentage — an overtime loss banks a point, so a 40-20-20 team
/// (100 pts, .500) stands above a 41-39-0 one (82 pts, .5125 by wins alone),
/// which is the order the league's own table uses.
fn win_pct(r: &StandingRow) -> f64 {
    let third = r.third.unwrap_or(0);
    let played = r.wins + r.losses + third;
    if played == 0 {
        return 0.0;
    }
    (r.wins as f64 + 0.5 * third as f64) / played as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(wins: u32, losses: u32, third: Option<u32>) -> StandingRow {
        StandingRow {
            abbr: "XX".into(),
            name: "Team".into(),
            wins,
            losses,
            third,
            third_label: if third.is_some() { "OTL" } else { "" },
        }
    }

    #[test]
    fn yards_to_goal_boundaries_for_both_possession_sides() {
        // Pin the formula at the yardLine extremes for both
        // sides. yardLine 0 = home goal line, 100 = away goal line; home
        // attacks 100, away attacks 0.
        assert_eq!(
            yards_to_goal(0, true),
            100,
            "home at its own goal line: 100 to go"
        );
        assert_eq!(yards_to_goal(50, true), 50, "midfield: 50 to go either way");
        assert_eq!(
            yards_to_goal(100, true),
            0,
            "home at the away goal line: 0 to go"
        );
        assert_eq!(
            yards_to_goal(0, false),
            0,
            "away at the home goal line: 0 to go"
        );
        assert_eq!(
            yards_to_goal(50, false),
            50,
            "midfield: 50 to go either way"
        );
        assert_eq!(
            yards_to_goal(100, false),
            100,
            "away at its own goal line: 100 to go"
        );
    }

    #[test]
    fn win_pct_counts_the_third_column_as_half_a_win() {
        // The NHL's own table: 40-20-20 is 100 points in 80 games (.625 of
        // the points available); 41-39-0 is 82 in 80. Sorting on wins alone
        // would flip them (.5125 > .500) and stand a worse team higher.
        let otl = row(40, 20, Some(20));
        let none = row(41, 39, Some(0));
        assert_eq!(win_pct(&otl), 0.625);
        assert_eq!(win_pct(&none), 0.5125);
        assert!(
            win_pct(&otl) > win_pct(&none),
            "100 points must stand above 82"
        );
        // Baseball/basketball keep no third column: plain wins over games.
        assert_eq!(win_pct(&row(81, 81, None)), 0.5);
        assert_eq!(win_pct(&row(58, 24, None)), 58.0 / 82.0);
        // Nobody has played: no divide by zero, no fake .000 ordering games.
        assert_eq!(win_pct(&row(0, 0, None)), 0.0);
    }
}
