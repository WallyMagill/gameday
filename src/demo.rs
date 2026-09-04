//! Seed data for `--demo` and `dump`: a full watch board, no network.
//! One live marquee game per league (NFL/NBA/MLB/NHL) pinned to Home,
//! plus slate games so league tabs have upcoming/final rows.

use crate::config::{Config, Pin};
use crate::domain::*;
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
        kind: PlayKind::Other,
        score_value: None,
    }
}

/// A play in a sport with no play clock: the stamp is the half-inning tag
/// (`B7`, `T8`) the `Play::period` field exists for. v3.1 left the demo's
/// baseball plays carrying invented `0:42` clocks, so `[B7]` — the one form
/// the mapper emits for MLB — never appeared in a capture.
fn inning_play(period: &str, team: &str, text: &str, scoring: bool) -> Play {
    Play {
        clock: String::new(),
        period: period.into(),
        team: team.into(),
        text: text.into(),
        scoring,
        kind: PlayKind::Other,
        score_value: None,
    }
}

pub fn demo_config() -> Config {
    Config {
        enabled_tabs: vec![League::Nfl, League::Nba, League::Mlb, League::Nhl, League::Epl],
        favorites: vec![],
        theme: crate::theme::current_name(),
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

/// A finished game: the board's tier-3 FINAL row is an abbr pair, a score, a
/// league tag and one headline, so that is all a slate filler needs.
fn final_game(
    id: &str,
    league: League,
    away: Team,
    home: Team,
    away_score: u16,
    home_score: u16,
    headline: &str,
) -> Game {
    Game {
        id: id.into(),
        league,
        away,
        home,
        away_score,
        home_score,
        status: Status::Final,
        period: "F".into(),
        scoring_plays: vec![play("", "", headline, true)],
        ..Game::default()
    }
}

/// A scheduled game: a start time, a network and a line — the three fields a
/// LATER row prints.
fn later_game(id: &str, league: League, away: Team, home: Team, net: &str, odds: &str) -> Game {
    Game {
        id: id.into(),
        league,
        away,
        home,
        status: Status::Pre,
        start: Some(time::macros::datetime!(2026-09-13 16:25 -4)),
        broadcast: Some(net.into()),
        odds: Some(odds.into()),
        ..Game::default()
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
    let gb = team("GB", "Green Bay", "Packers", "9-8", [24, 48, 40], [255, 184, 28], League::Nfl);
    let chi = team("CHI", "Chicago", "Bears", "7-10", [11, 22, 42], [200, 56, 3], League::Nfl);
    let nyj = team("NYJ", "New York", "Jets", "6-11", [18, 87, 64], [255, 255, 255], League::Nfl);
    let mia = team("MIA", "Miami", "Dolphins", "10-7", [0, 142, 151], [252, 76, 2], League::Nfl);
    let hou = team("HOU", "Houston", "Texans", "10-7", [3, 32, 47], [167, 25, 48], League::Nfl);
    let lar = team("LAR", "Los Angeles", "Rams", "10-7", [0, 53, 148], [255, 209, 0], League::Nfl);
    let no_ = team("NO", "New Orleans", "Saints", "5-12", [211, 188, 141], [16, 24, 31], League::Nfl);
    let car = team("CAR", "Carolina", "Panthers", "5-12", [0, 133, 202], [16, 24, 31], League::Nfl);
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
                // Football timeouts: KC burned one on the goal-line stand.
                // The zoom's matchup line is made of this field.
                timeouts: Some((2, 1)),
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
            // A second live NFL game so IN PLAY has a mixed-league list with
            // more than one football row — the A′ frame's own GB 13 CHI 10.
            Game {
                id: "nfl-live2".into(),
                league: League::Nfl,
                away: gb,
                home: chi,
                away_score: 13,
                home_score: 10,
                status: Status::Live,
                period: "Q3".into(),
                clock: "4:20".into(),
                situation: Some(Situation {
                    down_distance: "3rd & 2".into(),
                    possession: Some("GB".into()),
                    ball_on: Some("CHI 41".into()),
                    ..Default::default()
                }),
                last_plays: vec![
                    play("4:20", "GB", "Love scrambles for 6, short of the sticks", false),
                    play("5:02", "CHI", "Williams pass to Odunze for 14", false),
                ],
                timeouts: Some((3, 2)),
                linescore: vec![(3, 7), (7, 0), (3, 3)],
                broadcast: Some("FOX".into()),
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
            final_game("nfl-final2", League::Nfl, nyj, mia, 10, 13, "Tua finds Hill for the winner"),
            later_game("nfl-late2", League::Nfl, hou, lar, "CBS", "LAR -3  O/U 45.5"),
            later_game("nfl-late3", League::Nfl, no_, car, "CBS", "CAR -1  O/U 40.5"),
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
            timeouts: Some((3, 2)),
            broadcast: Some("TNT".into()),
            ..Game::default()
        }],
    );

    let nyy = team("NYY", "New York", "Yankees", "29-17", [220, 220, 230], [70, 110, 180], League::Mlb);
    let tor = team("TOR", "Toronto", "Blue Jays", "24-22", [70, 130, 220], [220, 220, 220], League::Mlb);
    let bos_mlb = team("BOS", "Boston", "Red Sox", "25-21", [189, 48, 57], [12, 35, 64], League::Mlb);
    let tex = team("TEX", "Texas", "Rangers", "23-23", [0, 50, 120], [192, 17, 31], League::Mlb);
    let sf_mlb = team("SF", "San Francisco", "Giants", "26-20", [253, 90, 30], [39, 37, 31], League::Mlb);
    let atl = team("ATL", "Atlanta", "Braves", "22-24", [19, 39, 79], [206, 17, 65], League::Mlb);
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
                // The zoom's baseball matchup line (spec §5) is made of these
                // three fields and nothing else.
                pitcher: Some("C. Schmidt".into()),
                batter: Some("A. Kirk".into()),
                due_up: vec!["D. Varsho".into(), "E. Clement".into(), "G. Springer".into()],
                ..Default::default()
            }),
            last_plays: vec![
                inning_play("B7", "TOR", "Vladimir Guerrero Jr. singles to right", false),
                inning_play("T7", "NYY", "Aaron Judge homers to left (18)  [5-3]", true),
                inning_play("T7", "NYY", "Jazz Chisholm Jr. walks", false),
                inning_play("B6", "TOR", "Bo Bichette strikes out swinging", false),
            ],
            meter: Some(Meter::Diamond { occupied: [true, false, false] }),
            // Seven innings played, plus the H/E the MLB linescore row adds.
            linescore: vec![(0, 1), (2, 0), (0, 0), (1, 1), (0, 0), (2, 1), (0, 0)],
            extras: Extras::Baseball { hits: Some((9, 7)), errors: Some((0, 1)) },
            broadcast: Some("SN".into()),
            ..Game::default()
        },
        // A mid-table live game: tied, mid-innings, no bonus — it sits below
        // the marquee games and above the finals, which is what a ranked list
        // needs to look ranked.
        Game {
            id: "mlb-live2".into(),
            league: League::Mlb,
            away: bos_mlb,
            home: tex,
            away_score: 2,
            home_score: 2,
            status: Status::Live,
            period: "TOP 6TH".into(),
            situation: Some(Situation {
                down_distance: "1 OUT · 0-1".into(),
                balls: Some(0),
                strikes: Some(1),
                outs: Some(1),
                on_base: Some([false; 3]),
                ..Default::default()
            }),
            last_plays: vec![inning_play("T6", "BOS", "Devers lines out to left", false)],
            meter: Some(Meter::Diamond { occupied: [false; 3] }),
            broadcast: Some("NESN".into()),
            ..Game::default()
        },
        final_game("mlb-final", League::Mlb, sf_mlb, atl, 7, 3, "Chapman homers twice"),
        ],
    );

    let edm = team("EDM", "Edmonton", "Oilers", "49-27", [252, 100, 30], [65, 105, 225], League::Nhl);
    let dal_nhl = team("DAL", "Dallas", "Stars", "52-21", [0, 200, 130], [220, 220, 220], League::Nhl);
    let bos_nhl = team("BOS", "Boston", "Bruins", "44-32", [252, 181, 20], [17, 17, 17], League::Nhl);
    let tor_nhl = team("TOR", "Toronto", "Maple Leafs", "46-30", [0, 32, 91], [220, 220, 220], League::Nhl);
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
        },
        final_game("nhl-final", League::Nhl, bos_nhl, tor_nhl, 4, 2, "Pastrnak two goals, Swayman 31 saves"),
        ],
    );

    let liv = team("LIV", "Liverpool", "Liverpool", "0-2-0", [211, 19, 23], [220, 220, 220], League::Epl);
    let ars = team("ARS", "London", "Arsenal", "1-1-0", [239, 1, 7], [220, 220, 220], League::Epl);
    let mci = team("MCI", "Manchester", "Man City", "2-0-0", [108, 171, 221], [220, 220, 220], League::Epl);
    let che = team("CHE", "London", "Chelsea", "1-0-1", [3, 70, 148], [220, 220, 220], League::Epl);
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
            // The soccer matchup line reads `Extras::Soccer::events`; without
            // it the zoom of an EPL game had nothing under the linescore.
            extras: Extras::Soccer {
                events: vec![
                    MatchEvent { minute: "51'".into(), kind: EventKind::Yellow, team: "ARS".into(), player: "Rice".into() },
                    MatchEvent { minute: "64'".into(), kind: EventKind::Goal, team: "ARS".into(), player: "Saka".into() },
                    MatchEvent { minute: "76'".into(), kind: EventKind::Goal, team: "LIV".into(), player: "Salah".into() },
                ],
            },
            broadcast: Some("NBC".into()),
            ..Game::default()
        },
        final_game("epl-final", League::Epl, mci, che, 2, 2, "Haaland levels it in the 88th"),
        ],
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

    /// The slate's shape is a gallery contract, not a detail. `dump`'s
    /// `board-broadcast` renders at DUMP_COLS x DUMP_ROWS (120x36) and the
    /// off-screen SCORES lane only appears there because the slate is big
    /// enough to overflow the frame: 15 games, with LATER pushed past the
    /// fold. Trimming a league (or a state) silently takes the lane back out
    /// of the gallery, which is the only place it is ever eyeballed.
    #[test]
    fn the_demo_slate_fills_the_gallery_frame_and_fires_the_off_screen_lane() {
        let boards = demo_boards();
        let all: Vec<&Game> = boards.values().flatten().collect();
        let count = |st: Status| all.iter().filter(|g| g.status == st).count();
        assert_eq!(all.len(), 15, "the slate is 15 games");
        assert_eq!(count(Status::Live), 7, "live games");
        assert_eq!(count(Status::Final), 5, "FINAL has five rows");
        assert_eq!(count(Status::Pre), 3, "LATER has three rows");
        // Every league the demo config enables carries at least one game.
        for league in demo_config().enabled_tabs {
            assert!(
                boards.get(&league).is_some_and(|g| !g.is_empty()),
                "{league:?} has no demo games"
            );
        }

        // …and at the gallery's own size the board overflows, so the lane
        // draws and names exactly the three LATER games it pushed off.
        let buf = crate::dump::render_demo_buffer(
            crate::dump::DUMP_COLS,
            crate::dump::DUMP_ROWS,
            0,
        )
        .unwrap();
        let row = |y: u16| -> String {
            (0..crate::dump::DUMP_COLS)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        // Last row is the footer legend; the lane sits directly above it.
        let lane = row(crate::dump::DUMP_ROWS - 2);
        assert!(
            lane.starts_with("SCORES") && lane.contains("3 OFF-SCREEN · 0 FINAL · 3 LATER"),
            "the gallery board must fire the off-screen lane, got {lane:?}"
        );
    }
}
