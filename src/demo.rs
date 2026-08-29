//! Seed data for `--demo` and `dump`: a full watch board, no network.
//! One live marquee game per league (NFL/NBA/MLB/NHL) pinned to Home,
//! plus slate games so league tabs have upcoming/final rows.

use crate::config::{Config, Pin};
use crate::domain::*;
use crate::tiles::packer::LayoutPref;
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
    }
}

fn play(clock: &str, team: &str, text: &str, scoring: bool) -> Play {
    Play {
        clock: clock.into(),
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
        theme: crate::theme::current_name().as_str().to_string(),
    }
}

pub fn demo_pins() -> Vec<Pin> {
    ["nfl-live", "nba-live", "mlb-live", "nhl-live"]
        .into_iter()
        .zip([League::Nfl, League::Nba, League::Mlb, League::Nhl])
        .map(|(id, league)| Pin {
            game_id: id.into(),
            league,
            final_at: None,
        })
        .collect()
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
                start_time: None,
                broadcast: Some("CBS".into()),
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
                start_time: Some("8:20 PM".into()),
                broadcast: Some("NBC".into()),
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
                start_time: None,
                broadcast: Some("FOX".into()),
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
            situation: None,
            last_plays: vec![
                play("4:38", "DEN", "Nikola Jokic makes layup (28 PTS)", false),
                play("5:02", "BOS", "Jayson Tatum 3pt shot (23 PTS)", true),
                play("5:28", "DEN", "Jamal Murray makes jumper (18 PTS)", false),
                play("5:45", "BOS", "Jrue Holiday steal", false),
            ],
            meter: Some(Meter::Lead { plus_minus: -7 }),
            start_time: None,
            broadcast: Some("TNT".into()),
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
                down_distance: "2 OUTS  1-2".into(),
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
            start_time: None,
            broadcast: Some("SN".into()),
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
            start_time: None,
            broadcast: Some("ESPN".into()),
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
            start_time: None,
            broadcast: Some("NBC".into()),
        }],
    );

    boards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_board_has_four_live_leagues_and_pins() {
        let boards = demo_boards();
        let pins = demo_pins();
        assert_eq!(pins.len(), 4);
        for pin in &pins {
            let board = boards.get(&pin.league).expect("board for pinned league");
            let game = board.iter().find(|g| g.id == pin.game_id).expect("pinned game");
            assert_eq!(game.status, Status::Live);
            assert!(!game.last_plays.is_empty());
            assert!(game.meter.is_some());
        }
    }
}
