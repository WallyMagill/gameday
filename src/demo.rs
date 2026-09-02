//! Seed data for `--demo` and `dump`: a full watch board, no network.
//! One live marquee game per league (NFL/NBA/MLB/NHL) pinned to Home,
//! plus slate games so league tabs have upcoming/final rows.

use crate::config::{Config, Pin};
use crate::domain::*;
use crate::config::LayoutPref;
use std::collections::HashMap;

fn team(
    abbr: &str,
    location: &str,
    name: &str,
    record: &str,
    color: [u8; 3],
    alt: [u8; 3],
    league: League,
) -> Team {
    Team {
        id: abbr.into(),
        abbr: abbr.into(),
        name: name.into(),
        location: location.into(),
        record: record.into(),
        color,
        alt_color: alt,
        logo_key: format!("{}/{}", league.slug(), abbr.to_lowercase()),
        rank: None,
    }
}

fn play(clock: &str, team: &str, text: &str, scoring: bool) -> Play {
    Play {
        clock: clock.into(),
        period: String::new(),
        team: team.into(),
        text: text.into(),
        scoring,
    }
}

pub fn demo_config() -> Config {
    Config {
        enabled_tabs: vec![League::Nfl, League::Nba, League::Mlb, League::Nhl, League::Epl],
        layout: LayoutPref::Auto,
        favorites: vec![],
        theme: crate::theme::current_name(),
        score_style: Default::default(),
        sort: Default::default(),
    }
}

pub fn demo_pins() -> Vec<Pin> {
    // One pin, not four: Home shows every live game on its own now, so pinning
    // the whole slate only puts a ⚑ on every tile in every capture. KC@TB is
    // the pin that reads as a choice.
    vec![Pin {
        game_id: "nfl-live".into(),
        league: League::Nfl,
        final_at: None,
    }]
}

/// A box score for the demo `nfl-live` game (KC 27 @ TB 24), shaped like
/// `map_stats` output from the summary endpoint: comparison rows in ESPN's
/// order with the labels the real feed uses, then one leader per team per
/// category. The dump's zoom-stats capture feeds from this so the STATS tab
/// shows KC/TB numbers and KC/TB players under KC/TB columns — the committed
/// `nfl_boxscore.json` is a real TEN@SEA game and stays the mapper's fixture.
pub fn demo_stats() -> GameStats {
    let row = |label: &str, away: &str, home: &str| StatRow {
        label: label.into(),
        away: away.into(),
        home: home.into(),
    };
    let leader = |team: &str, label: &str, text: &str| Leader {
        team: team.into(),
        label: label.into(),
        text: text.into(),
    };
    GameStats {
        rows: vec![
            row("1st Downs", "19", "21"),
            row("Passing 1st downs", "12", "13"),
            row("Rushing 1st downs", "5", "6"),
            row("1st downs from penalties", "2", "2"),
            row("3rd down efficiency", "6-11", "5-12"),
            row("4th down efficiency", "1-1", "0-2"),
            row("Total Plays", "58", "64"),
            row("Total Yards", "356", "341"),
            row("Yards per Play", "6.1", "5.3"),
            row("Total Drives", "10", "11"),
            row("Passing", "241", "228"),
            row("Comp/Att", "22/31", "24/38"),
            row("Yards per pass", "7.3", "5.7"),
            row("Interceptions thrown", "0", "1"),
            row("Sacks-Yards Lost", "2-14", "3-22"),
            row("Rushing", "115", "113"),
            row("Rushing Attempts", "25", "23"),
            row("Yards per rush", "4.6", "4.9"),
            row("Red Zone (Made-Att)", "2-4", "3-3"),
            row("Penalties", "5-40", "7-61"),
            row("Turnovers", "0", "1"),
            row("Possession", "28:41", "31:19"),
        ],
        leaders: vec![
            leader("KC", "Passing Yards", "P. Mahomes 22/31, 255 YDS, 2 TD"),
            leader("KC", "Rushing Yards", "I. Pacheco 17 CAR, 82 YDS, 1 TD"),
            leader("KC", "Receiving Yards", "T. Kelce 8 REC, 96 YDS, 1 TD"),
            leader("KC", "Sacks", "G. Karlaftis 2.0"),
            leader("KC", "Tackles", "N. Bolton 9"),
            leader("TB", "Passing Yards", "B. Mayfield 24/38, 250 YDS, 1 TD, 1 INT"),
            leader("TB", "Rushing Yards", "R. White 15 CAR, 71 YDS, 1 TD"),
            leader("TB", "Receiving Yards", "M. Evans 6 REC, 88 YDS, 1 TD"),
            leader("TB", "Sacks", "Y. Diaby 1.5"),
            leader("TB", "Tackles", "L. David 11"),
        ],
    }
}

pub fn demo_boards() -> HashMap<League, Vec<Game>> {
    let mut boards = HashMap::new();

    let kc = team("KC", "Kansas City", "Chiefs", "11-6", [227, 24, 55], [255, 184, 28], League::Nfl);
    let tb = team("TB", "Tampa Bay", "Buccaneers", "11-6", [255, 60, 40], [180, 180, 180], League::Nfl);
    let sf = team("SF", "San Francisco", "49ers", "12-5", [200, 60, 50], [230, 190, 130], League::Nfl);
    let sea = team("SEA", "Seattle", "Seahawks", "9-8", [105, 190, 40], [0, 90, 170], League::Nfl);
    let phi = team("PHI", "Philadelphia", "Eagles", "12-5", [0, 140, 130], [200, 200, 200], League::Nfl);
    let dal = team("DAL", "Dallas", "Cowboys", "10-7", [90, 130, 200], [180, 180, 180], League::Nfl);
    boards.insert(
        League::Nfl,
        vec![
            Game {
                id: "nfl-live".into(),
                league: League::Nfl,
                away: kc,
                home: tb,
                away_score: 27,
                home_score: 24,
                status: Status::Live,
                period: "Q4".into(),
                clock: "1:27".into(),
                situation: Some(Situation {
                    down_distance: "1st & Goal".into(),
                    possession: Some("KC".into()),
                    ball_on: Some("TB 3".into()),
                    ..Default::default()
                }),
                last_plays: vec![
                    play("1:27", "KC", "Patrick Mahomes pass to T. Kelce for 3 yards (1st & Goal)", false),
                    play("2:02", "TB", "Baker Mayfield sacked for -7 yards", false),
                    play("2:45", "KC", "Isiah Pacheco rush for 8 yards", false),
                    play("3:21", "KC", "Mahomes pass to Kelce, 12 yd TOUCHDOWN", true),
                ],
                meter: Some(Meter::RedZone { yards_to_goal: 3 }),
                // Q1..Q4 (away, home) — the zoom overview's linescore row.
                linescore: vec![(7, 3), (6, 14), (7, 0), (7, 7)],
                broadcast: Some("CBS".into()),
                ..Game::default()
            },
            Game {
                id: "nfl-pre".into(),
                league: League::Nfl,
                away: sf,
                home: sea,
                away_score: 0,
                home_score: 0,
                status: Status::Pre,
                period: String::new(),
                clock: String::new(),
                situation: None,
                last_plays: vec![],
                meter: None,
                start: Some(time::macros::datetime!(2026-09-13 20:20 -4)),
                broadcast: Some("NBC".into()),
                odds: Some("SF -2.5  O/U 44.5".into()),
                ..Game::default()
            },
            Game {
                id: "nfl-final".into(),
                league: League::Nfl,
                away: phi,
                home: dal,
                away_score: 28,
                home_score: 17,
                status: Status::Final,
                period: "F".into(),
                clock: String::new(),
                situation: None,
                last_plays: vec![],
                meter: None,
                broadcast: Some("FOX".into()),
                ..Game::default()
            },
        ],
    );

    let den = team("DEN", "Denver", "Nuggets", "53-29", [254, 197, 36], [30, 60, 110], League::Nba);
    let bos = team("BOS", "Boston", "Celtics", "58-24", [0, 180, 90], [220, 220, 220], League::Nba);
    boards.insert(
        League::Nba,
        vec![Game {
            id: "nba-live".into(),
            league: League::Nba,
            away: den,
            home: bos,
            away_score: 88,
            home_score: 81,
            status: Status::Live,
            period: "Q3".into(),
            clock: "4:38".into(),
            // Real feeds never carry a shot clock (see provider::map); the
            // demo supplies one so the boxed amber chip is visible.
            situation: Some(Situation {
                shot_clock: Some(24),
                ..Default::default()
            }),
            last_plays: vec![
                play("4:38", "DEN", "Nikola Jokic makes layup (28 PTS)", false),
                play("5:02", "BOS", "Jayson Tatum 3pt shot (23 PTS)", true),
                play("5:28", "DEN", "Jamal Murray makes jumper (18 PTS)", false),
                play("5:45", "BOS", "Jrue Holiday steal", false),
            ],
            meter: Some(Meter::Lead { plus_minus: -7 }),
            broadcast: Some("TNT".into()),
            ..Game::default()
        }],
    );

    let nyy = team("NYY", "New York", "Yankees", "29-17", [220, 220, 230], [70, 110, 180], League::Mlb);
    let tor = team("TOR", "Toronto", "Blue Jays", "24-22", [70, 130, 220], [220, 220, 220], League::Mlb);
    boards.insert(
        League::Mlb,
        vec![Game {
            id: "mlb-live".into(),
            league: League::Mlb,
            away: nyy,
            home: tor,
            away_score: 5,
            home_score: 3,
            status: Status::Live,
            period: "BOT 7TH".into(),
            clock: String::new(),
            situation: Some(Situation {
                down_distance: "2 OUT · 1-2".into(),
                balls: Some(1),
                strikes: Some(2),
                outs: Some(2),
                on_base: Some([true, false, false]),
                ..Default::default()
            }),
            last_plays: vec![
                play("0:42", "NYY", "Aaron Judge homers to left (18)  [5-3]", true),
                play("1:15", "TOR", "Vladimir Guerrero Jr. single to right", false),
                play("1:48", "NYY", "Jazz Chisholm Jr. walks", false),
                play("2:21", "TOR", "Bo Bichette strikes out", false),
            ],
            meter: Some(Meter::Diamond { occupied: [true, false, false] }),
            // Seven innings played, plus the H/E the MLB linescore row adds.
            linescore: vec![(0, 1), (2, 0), (0, 0), (1, 1), (0, 0), (2, 1), (0, 0)],
            extras: Extras::Baseball { hits: Some((9, 7)), errors: Some((0, 1)) },
            broadcast: Some("SN".into()),
            ..Game::default()
        }],
    );

    let edm = team("EDM", "Edmonton", "Oilers", "49-27", [252, 100, 30], [65, 105, 225], League::Nhl);
    let dal_nhl = team("DAL", "Dallas", "Stars", "52-21", [0, 200, 130], [220, 220, 220], League::Nhl);
    boards.insert(
        League::Nhl,
        vec![Game {
            id: "nhl-live".into(),
            league: League::Nhl,
            away: edm,
            home: dal_nhl,
            away_score: 3,
            home_score: 2,
            status: Status::Live,
            period: "2ND".into(),
            clock: "1:03".into(),
            situation: None,
            last_plays: vec![
                play("1:03", "EDM", "Leon Draisaitl snap shot GOAL (32)  [3-2]", true),
                play("2:37", "DAL", "Roope Hintz tip-in goal  [2-2]", true),
                play("4:11", "EDM", "Evan Bouchard shot on goal", false),
                play("5:09", "DAL", "Jamie Benn hit", false),
            ],
            meter: Some(Meter::Penalty { team_abbr: "DAL".into(), seconds: 42 }),
            broadcast: Some("ESPN".into()),
            ..Game::default()
        }],
    );

    let liv = team("LIV", "Liverpool", "Liverpool", "0-2-0", [211, 19, 23], [220, 220, 220], League::Epl);
    let ars = team("ARS", "London", "Arsenal", "1-1-0", [239, 1, 7], [220, 220, 220], League::Epl);
    boards.insert(
        League::Epl,
        vec![Game {
            id: "epl-live".into(),
            league: League::Epl,
            away: ars,
            home: liv,
            away_score: 1,
            home_score: 2,
            status: Status::Live,
            period: "78'".into(),
            clock: String::new(),
            situation: None,
            last_plays: vec![
                play("76'", "LIV", "Mohamed Salah right-footed GOAL from the box  [2-1]", true),
                play("64'", "ARS", "Bukayo Saka curls one in from the edge  [1-1]", true),
                play("58'", "LIV", "Virgil van Dijk header cleared off the line", false),
                play("51'", "ARS", "Declan Rice booked for a late challenge", false),
            ],
            meter: None,
            broadcast: Some("NBC".into()),
            ..Game::default()
        }],
    );

    // The scoring feed (alerts, top plays, ticker, zoom SCORING) reads
    // `Game.scoring_plays`, oldest-first. The demo authors its plays
    // newest-first, so mirror the scoring rows into it.
    for game in boards.values_mut().flatten() {
        game.scoring_plays = game
            .last_plays
            .iter()
            .filter(|p| p.scoring)
            .rev()
            .cloned()
            .collect();
    }
    boards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_board_has_four_live_leagues_and_pins() {
        let boards = demo_boards();
        // The four scripted live games are still there; only the pin list shrank.
        for (league, id) in [
            (League::Nfl, "nfl-live"),
            (League::Nba, "nba-live"),
            (League::Mlb, "mlb-live"),
            (League::Nhl, "nhl-live"),
        ] {
            let board = boards.get(&league).expect("board for live league");
            let game = board.iter().find(|g| g.id == id).expect("live game");
            assert_eq!(game.status, Status::Live);
        }
        // Home lists every live game unpinned, so the demo pins exactly one —
        // otherwise every gallery tile wears a ⚑ and the flag says nothing.
        let pins = demo_pins();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].game_id, "nfl-live");
        for pin in &pins {
            let board = boards.get(&pin.league).expect("board for pinned league");
            let game = board.iter().find(|g| g.id == pin.game_id).expect("pinned game");
            assert_eq!(game.status, Status::Live);
            assert!(!game.last_plays.is_empty());
            assert!(game.meter.is_some());
        }
    }
}
