//! Demo simulation driver: advances the `demo` boards through a fixed,
//! scripted event stream — one tick per second in `--demo`, pure ticks in
//! `dump --tick N`. No RNG anywhere: the board at tick N is a function of N.
//!
//! `demo::demo_boards()` stays the tick-0 state; every beat below mutates
//! those games in place (clocks run, plays append, KC scores the TD at
//! [`KC_TD_TICK`], the NBA lead flips, the MLB inning turns over, the NHL
//! penalty runs out) and the mutated boards flow through the same
//! `Msg::Boards` channel the real provider uses, so the UI path is identical
//! between demo and live.

use crate::demo;
use crate::domain::*;
use std::collections::HashMap;

/// Seconds into the demo when KC scores the touchdown — the primary fixture
/// for the score-flash animation work.
pub const KC_TD_TICK: u64 = 15;
/// Tick when the MLB half-inning turns over (BOT 7TH -> TOP 8TH).
pub const MLB_INNING_TICK: u64 = 24;
/// Tick when the NHL penalty (42s at tick 0, 1s per tick) expires.
pub const NHL_PENALTY_CLEAR_TICK: u64 = 42;

/// Remaining game clocks at tick 0, in seconds (NFL "1:27", NBA "4:38",
/// NHL "1:03" from the demo board).
const NFL_CLOCK0: u64 = 87;
const NBA_CLOCK0: u64 = 278;
const NHL_CLOCK0: u64 = 63;

pub struct Simulator {
    boards: HashMap<League, Vec<Game>>,
    tick: u64,
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Simulator {
    pub fn new() -> Self {
        Self {
            boards: demo::demo_boards(),
            tick: 0,
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn boards(&self) -> &HashMap<League, Vec<Game>> {
        &self.boards
    }

    /// Advance one tick (one simulated second) and apply that tick's beats.
    pub fn step(&mut self) {
        self.tick += 1;
        let t = self.tick;
        self.with_game(League::Nfl, "nfl-live", |g| step_nfl(g, t));
        self.with_game(League::Nba, "nba-live", |g| step_nba(g, t));
        self.with_game(League::Mlb, "mlb-live", |g| step_mlb(g, t));
        self.with_game(League::Nhl, "nhl-live", |g| step_nhl(g, t));
        self.with_game(League::Epl, "epl-live", |g| step_epl(g, t));
    }

    /// Advance to an absolute tick. Pure: N steps applied in order, no clock.
    pub fn advance_to(&mut self, tick: u64) {
        while self.tick < tick {
            self.step();
        }
    }

    /// The full board state at tick N, computed from scratch.
    pub fn boards_at(tick: u64) -> HashMap<League, Vec<Game>> {
        let mut sim = Self::new();
        sim.advance_to(tick);
        sim.boards
    }

    fn with_game(&mut self, league: League, id: &str, f: impl FnOnce(&mut Game)) {
        if let Some(game) = self
            .boards
            .get_mut(&league)
            .and_then(|b| b.iter_mut().find(|g| g.id == id))
        {
            f(game);
        }
    }
}

fn fmt_clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Newest play goes on top; the tile shows a handful, keep a small window.
fn push_play(g: &mut Game, team: &str, text: &str, scoring: bool) {
    let clock = if g.clock.is_empty() {
        g.period.clone()
    } else {
        g.clock.clone()
    };
    g.last_plays.insert(
        0,
        Play {
            clock,
            period: String::new(),
            team: team.into(),
            text: text.into(),
            scoring,
        },
    );
    g.last_plays.truncate(6);
}

/// KC @ TB, Q4: goal-line stand, TD at [`KC_TD_TICK`], PAT, kickoff, TB's
/// answer drive.
fn step_nfl(g: &mut Game, t: u64) {
    g.clock = fmt_clock(NFL_CLOCK0.saturating_sub(t));
    match t {
        8 => {
            push_play(g, "KC", "Mahomes pass to Rice incomplete (2nd & Goal)", false);
            if let Some(sit) = &mut g.situation {
                sit.down_distance = "2nd & Goal".into();
            }
        }
        KC_TD_TICK => {
            g.away_score += 6; // 27 -> 33
            push_play(g, "KC", "Mahomes pass to Kelce, 3 yd TOUCHDOWN", true);
            g.meter = None;
            g.situation = None;
        }
        17 => {
            g.away_score += 1; // 33 -> 34
            push_play(g, "KC", "Harrison Butker extra point is GOOD", false);
        }
        22 => {
            push_play(g, "KC", "Butker kicks off, touchback", false);
            g.situation = Some(Situation {
                down_distance: "1st & 10".into(),
                possession: Some("TB".into()),
                ball_on: Some("TB 30".into()),
                ..Default::default()
            });
        }
        35 => {
            push_play(g, "TB", "Mayfield pass to Evans for 18 yards", false);
            if let Some(sit) = &mut g.situation {
                sit.ball_on = Some("TB 48".into());
            }
        }
        _ => {}
    }
}

/// DEN @ BOS, Q3: Boston chips at a 7-point deficit and takes the lead at
/// tick 30 — the Lead meter flips sign.
fn step_nba(g: &mut Game, t: u64) {
    g.clock = fmt_clock(NBA_CLOCK0.saturating_sub(t));
    // Scripted 24-second shot clock: counts down and resets each cycle.
    if let Some(sit) = &mut g.situation {
        sit.shot_clock = Some(24 - (t % 24) as u8);
    }
    let scored = match t {
        6 => {
            g.home_score += 3;
            push_play(g, "BOS", "Jayson Tatum 3pt shot (26 PTS)", true);
            true
        }
        12 => {
            g.away_score += 2;
            push_play(g, "DEN", "Nikola Jokic makes layup (30 PTS)", false);
            true
        }
        18 => {
            g.home_score += 3;
            push_play(g, "BOS", "Derrick White 3pt from the corner", true);
            true
        }
        24 => {
            g.home_score += 2;
            push_play(g, "BOS", "Jaylen Brown driving dunk", false);
            true
        }
        30 => {
            g.home_score += 3;
            push_play(g, "BOS", "Tatum pull-up 3pt — BOS takes the lead", true);
            true
        }
        _ => false,
    };
    if scored {
        g.meter = Some(Meter::Lead {
            plus_minus: g.home_score as i16 - g.away_score as i16,
        });
    }
}

/// NYY @ TOR: the count runs full-ish, the third out ends the 7th at
/// [`MLB_INNING_TICK`] (label flips, diamond and count reset), a leadoff
/// single starts the 8th.
fn step_mlb(g: &mut Game, t: u64) {
    match t {
        12 => {
            if let Some(sit) = &mut g.situation {
                sit.balls = Some(2); // 1-2 -> 2-2
                if let Some(h) = sit.mlb_count_headline() {
                    sit.down_distance = h;
                }
            }
        }
        MLB_INNING_TICK => {
            push_play(g, "TOR", "Alejandro Kirk grounds out to short — inning over", false);
            g.period = "TOP 8TH".into();
            g.situation = Some(Situation {
                balls: Some(0),
                strikes: Some(0),
                outs: Some(0),
                on_base: Some([false; 3]),
                down_distance: "0 OUTS  0-0".into(),
                ..Default::default()
            });
            g.meter = Some(Meter::Diamond { occupied: [false; 3] });
        }
        38 => {
            push_play(g, "NYY", "Anthony Volpe singles to center", false);
            if let Some(sit) = &mut g.situation {
                sit.on_base = Some([true, false, false]);
            }
            g.meter = Some(Meter::Diamond { occupied: [true, false, false] });
        }
        _ => {}
    }
}

/// EDM @ DAL, 2ND: the DAL penalty (42s) runs down 1s per tick and clears at
/// [`NHL_PENALTY_CLEAR_TICK`]; Dallas ties it late.
fn step_nhl(g: &mut Game, t: u64) {
    g.clock = fmt_clock(NHL_CLOCK0.saturating_sub(t));
    if let Some(Meter::Penalty { seconds, .. }) = &mut g.meter {
        if *seconds > 1 {
            *seconds -= 1;
        } else {
            g.meter = None;
            push_play(g, "DAL", "Penalty expires — Stars back to full strength", false);
        }
    }
    match t {
        10 => push_play(g, "EDM", "Connor McDavid wrist shot, save Oettinger", false),
        50 => {
            g.home_score += 1; // 2 -> 3
            push_play(g, "DAL", "Wyatt Johnston snap shot GOAL (24)  [3-3]", true);
        }
        _ => {}
    }
}

/// ARS @ LIV: the match minute advances every 30 ticks; Liverpool puts it
/// away late.
fn step_epl(g: &mut Game, t: u64) {
    g.period = format!("{}'", 78 + t / 30);
    if t == 55 {
        g.home_score += 1; // 2 -> 3
        push_play(g, "LIV", "Dominik Szoboszlai smashes one in off the bar  [3-1]", true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game<'a>(boards: &'a HashMap<League, Vec<Game>>, league: League, id: &str) -> &'a Game {
        boards
            .get(&league)
            .and_then(|b| b.iter().find(|g| g.id == id))
            .unwrap_or_else(|| panic!("missing {id} in {league:?} board"))
    }

    #[test]
    fn tick_zero_is_the_demo_board() {
        assert_eq!(Simulator::boards_at(0), demo::demo_boards());
    }

    #[test]
    fn same_tick_same_board() {
        assert_eq!(Simulator::boards_at(45), Simulator::boards_at(45));
        // Stepping incrementally equals computing from scratch.
        let mut sim = Simulator::new();
        sim.advance_to(20);
        sim.advance_to(45);
        assert_eq!(*sim.boards(), Simulator::boards_at(45));
    }

    #[test]
    fn kc_td_changes_score_and_appends_scoring_play() {
        let before = Simulator::boards_at(KC_TD_TICK - 1);
        let b = game(&before, League::Nfl, "nfl-live");
        assert_eq!((b.away_score, b.home_score), (27, 24));

        let after = Simulator::boards_at(KC_TD_TICK);
        let g = game(&after, League::Nfl, "nfl-live");
        assert_eq!((g.away_score, g.home_score), (33, 24));
        let td = &g.last_plays[0];
        assert_eq!(td.team, "KC");
        assert!(td.scoring, "TD play must be a scoring play: {td:?}");
        assert!(td.text.contains("TOUCHDOWN"), "{td:?}");
        assert_eq!(td.clock, fmt_clock(NFL_CLOCK0 - KC_TD_TICK));
        assert_eq!(g.clock, td.clock, "play stamped with the live clock");
        assert_eq!(g.meter, None, "red zone clears after the TD");
    }

    #[test]
    fn nba_lead_meter_flips_sign() {
        let start = Simulator::boards_at(0);
        let after = Simulator::boards_at(30);
        let m0 = game(&start, League::Nba, "nba-live").meter.clone();
        let m1 = game(&after, League::Nba, "nba-live").meter.clone();
        assert_eq!(m0, Some(Meter::Lead { plus_minus: -7 }));
        assert_eq!(m1, Some(Meter::Lead { plus_minus: 2 }));
        let g = game(&after, League::Nba, "nba-live");
        assert_eq!((g.away_score, g.home_score), (90, 92));
    }

    #[test]
    fn late_tick_advances_mlb_inning_and_clears_nhl_penalty() {
        let boards = Simulator::boards_at(45);

        let mlb = game(&boards, League::Mlb, "mlb-live");
        assert_eq!(mlb.period, "TOP 8TH");
        let sit = mlb.situation.as_ref().expect("situation after turnover");
        assert_eq!(sit.outs, Some(0));
        assert_eq!((sit.balls, sit.strikes), (Some(0), Some(0)));

        let nhl = game(&boards, League::Nhl, "nhl-live");
        assert_eq!(nhl.meter, None, "penalty expired");
        assert!(nhl
            .last_plays
            .iter()
            .any(|p| p.text.contains("full strength")));

        // Clocks ran the whole way down deterministically.
        let nfl = game(&boards, League::Nfl, "nfl-live");
        assert_eq!(nfl.clock, fmt_clock(NFL_CLOCK0 - 45));
    }

    #[test]
    fn penalty_counts_down_before_clearing() {
        let boards = Simulator::boards_at(NHL_PENALTY_CLEAR_TICK - 1);
        let nhl = game(&boards, League::Nhl, "nhl-live");
        assert_eq!(
            nhl.meter,
            Some(Meter::Penalty { team_abbr: "DAL".into(), seconds: 1 })
        );
    }

    #[test]
    fn slate_games_are_untouched() {
        let boards = Simulator::boards_at(60);
        let demo = demo::demo_boards();
        assert_eq!(
            game(&boards, League::Nfl, "nfl-pre"),
            game(&demo, League::Nfl, "nfl-pre")
        );
        assert_eq!(
            game(&boards, League::Nfl, "nfl-final"),
            game(&demo, League::Nfl, "nfl-final")
        );
    }
}
