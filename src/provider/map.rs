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

fn period_label(period: i64) -> String {
    match period {
        0 => String::new(),
        1..=4 => format!("Q{period}"),
        _ => "OT".into(),
    }
}

fn team_from(league: League, v: &Value) -> Option<Team> {
    let id = v.get("id")?.as_str()?.to_string();
    let abbr = v.get("abbreviation")?.as_str()?.to_string();
    Some(Team {
        id,
        logo_key: format!("{}/{}", league.slug(), abbr.to_lowercase()),
        name: v.get("displayName").and_then(|x| x.as_str()).unwrap_or(&abbr).to_string(),
        color: hex_color(v.get("color").and_then(|x| x.as_str()).unwrap_or("")),
        alt_color: hex_color(v.get("alternateColor").and_then(|x| x.as_str()).unwrap_or("")),
        abbr,
    })
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
        let clock = st["displayClock"].as_str().unwrap_or("").to_string();
        let period = period_label(st["period"].as_i64().unwrap_or(0));
        let comps = comp.get("competitors").and_then(|c| c.as_array()).ok_or(MapError::Missing("competitors"))?;
        let mut home = None;
        let mut away = None;
        let mut home_score = 0u16;
        let mut away_score = 0u16;
        for c in comps {
            let team = team_from(league, &c["team"]).ok_or(MapError::Missing("team"))?;
            let score = c["score"].as_str().unwrap_or("0").parse().unwrap_or(0);
            match c["homeAway"].as_str() {
                Some("home") => { home_score = score; home = Some(team); }
                _ => { away_score = score; away = Some(team); }
            }
        }
        let home = home.ok_or(MapError::Missing("home"))?;
        let away = away.ok_or(MapError::Missing("away"))?;
        let sit_v = &comp["situation"];
        let situation = if sit_v.is_object() {
            let poss_id = sit_v["possession"].as_str();
            let possession = poss_id.and_then(|pid| {
                if home.id == pid { Some(home.abbr.clone()) }
                else if away.id == pid { Some(away.abbr.clone()) }
                else { None }
            });
            Some(Situation {
                down_distance: sit_v["downDistanceText"].as_str().unwrap_or("").to_string(),
                possession,
                ball_on: sit_v["possessionText"].as_str().map(|s| s.to_string()),
            })
        } else {
            None
        };
        let mut last_plays = Vec::new();
        if let Some(text) = sit_v["lastPlay"]["text"].as_str() {
            last_plays.push(Play {
                clock: sit_v["lastPlay"]["clock"]["displayValue"].as_str().unwrap_or(&clock).to_string(),
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
        out.push(Game {
            id, league, home, away, home_score, away_score, status, period, clock,
            situation, last_plays, meter: None, start_time, broadcast,
        });
    }
    Ok(out)
}

pub fn map_summary(json: &str) -> Result<Summary, MapError> {
    let v: Value = serde_json::from_str(json)?;
    let scoring_plays = v["scoringPlays"].as_array().cloned().unwrap_or_default()
        .iter()
        .filter_map(|p| {
            Some(Play {
                clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                text: p["text"].as_str()?.to_string(),
                scoring: true,
            })
        })
        .collect();
    let mut plays = Vec::new();
    if let Some(prev) = v["drives"]["previous"].as_array() {
        for d in prev {
            if let Some(ps) = d["plays"].as_array() {
                for p in ps {
                    if let Some(text) = p["text"].as_str() {
                        plays.push(Play {
                            clock: p["clock"]["displayValue"].as_str().unwrap_or("").to_string(),
                            text: text.to_string(),
                            scoring: p["scoringPlay"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
        }
    }
    if plays.len() > 8 {
        plays = plays.split_off(plays.len() - 8);
    }
    plays.reverse();
    Ok(Summary { last_plays: plays, scoring_plays, meter: None })
}
