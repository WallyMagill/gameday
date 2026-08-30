use crate::domain::*;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing field {0}")]
    Missing(&'static str),
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

fn team_from(league: League, v: &Value) -> Option<Team> {
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

pub fn map_scoreboard(league: League, json: &str) -> Result<Vec<Game>, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let events = v.get("events").and_then(|e| e.as_array()).ok_or(MapError::Missing("events"))?;
    let mut out = Vec::new();
    for ev in events {
        let id = ev.get("id").and_then(|x| x.as_str()).ok_or(MapError::Missing("id"))?.to_string();
        let start_time = ev.get("date").and_then(|x| x.as_str()).map(|s| s.to_string());
        let comp = ev.get("competitions").and_then(|c| c.as_array()).and_then(|a| a.first())
            .ok_or(MapError::Missing("competitions"))?;
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
        // period label; a raw "0:00" next to them is noise.
        let clock = match league {
            League::Mlb | League::Epl | League::Mls => String::new(),
            _ => display_clock.to_string(),
        };
        let comps = comp.get("competitors").and_then(|c| c.as_array()).ok_or(MapError::Missing("competitors"))?;
        let mut home = None;
        let mut away = None;
        let mut home_score = 0u16;
        let mut away_score = 0u16;
        for c in comps {
            let mut team = team_from(league, &c["team"]).ok_or(MapError::Missing("team"))?;
            team.record = record_from(c);
            // NHL shots on goal: skipped — no NHL fixture exists and the live
            // scoreboard (2026-08-29, all preseason `pre`) had competitors
            // with `statistics: []`, so the field name couldn't be verified.
            let score = c["score"].as_str().unwrap_or("0").parse().unwrap_or(0);
            match c["homeAway"].as_str() {
                Some("home") => { home_score = score; home = Some(team); }
                _ => { away_score = score; away = Some(team); }
            }
        }
        let home = home.ok_or(MapError::Missing("home"))?;
        let away = away.ok_or(MapError::Missing("away"))?;
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
                // Compose the headline: "2 OUTS  1-2".
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
        let mut last_plays = Vec::new();
        if let Some(text) = sit_v["lastPlay"]["text"].as_str() {
            // Attribute to the team ESPN credits on the play; fall back to
            // the possessing team when the play carries no team.
            let team = abbr_for_id(sit_v["lastPlay"]["team"]["id"].as_str())
                .or_else(|| situation.as_ref().and_then(|s| s.possession.clone()))
                .unwrap_or_default();
            last_plays.push(Play {
                clock: sit_v["lastPlay"]["clock"]["displayValue"].as_str().unwrap_or(&clock).to_string(),
                team,
                text: text.to_string(),
                scoring: false,
            });
        }
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
        out.push(Game {
            id, league, home, away, home_score, away_score, status, period, clock,
            situation, last_plays, meter, start_time, broadcast,
        });
    }
    Ok(out)
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
                plays.push(Play {
                    clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                    team: team_of(p),
                    text: text.to_string(),
                    scoring: p["scoringPlay"].as_bool().unwrap_or(false),
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
    if plays.len() > 8 {
        plays = plays.split_off(plays.len() - 8);
    }
    plays.reverse();
    // Meter stays None here on purpose: the scoreboard mapping owns meters and
    // App::merge_summary never reads a summary meter, so mapping one would be
    // dead data pretending to be live.
    Ok(Summary { last_plays: plays, scoring_plays, meter: None })
}
