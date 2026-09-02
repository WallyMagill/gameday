//! Watchability: which game deserves the hero and the hot mark. Pure
//! functions of the Game — the clock grammar strings come straight from the
//! mapper (fixtures verified per league), and an unparsed string scores 0 so
//! bad data can never lead the board (spec §2).
use crate::domain::{Game, League, Meter, Status};
use std::collections::HashMap;
use time::OffsetDateTime;

/// One game's watchability verdict. `score` orders the board; `hot` drives
/// the 2-state mark; `chip` is the hero/state label ("RED ZONE", "2-MIN",
/// "TYING RUN ON 3RD", "BASES LOADED", "STOPPAGE", …) or None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub score: u32,
    pub hot: bool,
    pub chip: Option<&'static str>,
}

/// Sport scale for "one score": margins at or under this are maximally close.
/// football 8 (one TD+2), basketball 10 (~3 possessions), hockey/soccer 1
/// (single goal ties it), baseball 3 (a 3-run homer ties it — save-situation scale).
fn one_score(league: League) -> u32 {
    match league {
        League::Nfl | League::Cfb => 8,
        League::Nba | League::Wnba | League::Cbb => 10,
        League::Nhl | League::Epl | League::Mls => 1,
        League::Mlb => 3,
    }
}

pub fn closeness(league: League, margin: u32) -> u32 {
    let scale = one_score(league);
    if margin == 0 {
        return 100;
    }
    // 100 at margin 0, ~60 at one score, → 0 by four scores. Linear enough.
    100u32.saturating_sub(margin.saturating_mul(40) / scale.max(1))
}

/// Elapsed fraction of the game, 0..=100, from the label grammar the mapper
/// emits. Returns 0 for anything it cannot parse (never tier-1 on bad data).
pub fn lateness(league: League, period: &str, clock: &str) -> u32 {
    match league {
        League::Nfl | League::Cfb | League::Nba | League::Wnba => {
            let q = match period {
                "Q1" => 1,
                "Q2" => 2,
                "Q3" => 3,
                "Q4" => 4,
                "OT" => 5,
                _ => return 0,
            };
            let qlen = quarter_len(league);
            // Unparseable clock ("junk", "") reads as "just started" (played
            // = 0), not "fully elapsed" — a bad clock string must never
            // inflate lateness (gap R23).
            let played = clock_secs(clock).map(|s| qlen - s.min(qlen)).unwrap_or(0);
            (((q - 1) as u32 * qlen + played) * 100 / (4 * qlen)).min(100)
        }
        League::Cbb => {
            let h = match period {
                "1ST HALF" => 1,
                "2ND HALF" => 2,
                "OT" => 3,
                _ => return 0,
            };
            let hl = 20 * 60; // college basketball halves are 20 minutes.
            let played = clock_secs(clock).map(|s| hl - s.min(hl)).unwrap_or(0);
            (((h - 1) as u32 * hl + played) * 100 / (2 * hl)).min(100)
        }
        League::Nhl => {
            let p = match period {
                "1ST" => 1,
                "2ND" => 2,
                "3RD" => 3,
                "OT" | "SO" => 4,
                _ => return 0,
            };
            let pl = 20 * 60; // NHL periods are 20 minutes.
            let played = clock_secs(clock).map(|s| pl - s.min(pl)).unwrap_or(0);
            (((p - 1) as u32 * pl + played) * 100 / (3 * pl)).min(100)
        }
        League::Mlb => {
            // "TOP 2ND" / "BOT 9TH" / "MID 5TH" / "END 8TH"
            let mut it = period.split_whitespace();
            let (half, num) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            let inning: u32 = num
                .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .parse()
                .ok()
                .unwrap_or(0);
            if inning == 0 {
                return 0;
            }
            let half_add = match half {
                "TOP" => 0,
                "MID" | "BOT" => 1,
                "END" => 2,
                _ => return 0,
            };
            // 18-half scale: 9 innings * 2 halves. inning=8 half=BOT(1) -> 77.
            (((inning - 1) * 2 + half_add) * 100 / 18).min(100)
        }
        League::Epl | League::Mls => {
            // "63'" / "90'+3'" — stoppage pins to 95+ (95 = maximal chip
            // threshold; +1 per stoppage minute, capped at 100).
            let base: u32 = period
                .split('\'')
                .next()
                .unwrap_or("")
                .parse()
                .ok()
                .unwrap_or(0);
            if base == 0 {
                return 0;
            }
            if period.contains('+') || base >= 90 {
                return 95 + (base.saturating_sub(90)).min(5);
            }
            // base is <90 here (the >=90 branch above already returned), so
            // no cap is needed: at base=89 this is 89*100/94 = 94, still
            // under the 95 stoppage floor set above.
            base * 100 / 94
        }
    }
}

fn quarter_len(league: League) -> u32 {
    match league {
        League::Nba => 12 * 60,  // NBA quarters are 12 minutes.
        League::Wnba => 10 * 60, // WNBA quarters are 10 minutes.
        _ => 15 * 60,            // NFL/CFB quarters are 15 minutes.
    }
}

/// "1:52" → Some(112); an unparseable clock ("junk", "") → None, so callers
/// can treat it as "clock unknown" (played = 0, this segment just started)
/// rather than silently reading it as fully elapsed (gap R23).
fn clock_secs(clock: &str) -> Option<u32> {
    let mut it = clock.split(':');
    match (
        it.next().and_then(|m| m.parse::<u32>().ok()),
        it.next().and_then(|s| s.parse::<u32>().ok()),
    ) {
        (Some(m), Some(s)) => Some(m * 60 + s),
        _ => None,
    }
}

// `_now` is unused today — kept in the signature so a later time-of-day
// weighting (e.g. late-night games ranked down) doesn't need to change the
// public API.
pub fn watchability(g: &Game, _now: OffsetDateTime) -> Watch {
    if g.status != Status::Live {
        return Watch {
            score: 0,
            hot: false,
            chip: None,
        };
    }
    let margin = g.home_score.abs_diff(g.away_score) as u32;
    let l = lateness(g.league, &g.period, &g.clock);
    let c = closeness(g.league, margin);
    let mut score = l * c / 100;
    let mut hot = false;
    let mut chip: Option<&'static str> = None;

    macro_rules! bonus {
        ($b:expr, $ch:expr) => {
            score += $b;
            hot = true;
            if chip.is_none() {
                chip = $ch;
            }
        };
    }

    match g.league {
        League::Nfl | League::Cfb => {
            // Red zone bonus: spec §2 table.
            if matches!(g.meter, Some(Meter::RedZone { .. })) {
                bonus!(40, Some("RED ZONE"));
            }
            // 120s = the two-minute warning window, Q2/Q4 only. An
            // unparseable clock (None) never triggers this — only a
            // confirmed reading under 2:00 does.
            if matches!(g.period.as_str(), "Q2" | "Q4") {
                if let Some(secs) = clock_secs(&g.clock) {
                    if secs > 0 && secs <= 120 {
                        bonus!(30, Some("2-MIN"));
                    }
                }
            }
        }
        League::Mlb => {
            let bases = g
                .situation
                .as_ref()
                .and_then(|s| s.on_base)
                .unwrap_or([false; 3]);
            if bases == [true, true, true] {
                bonus!(30, Some("BASES LOADED"));
            }
            // Tying/go-ahead run on base, 8th inning or later (77 = 8th-inning
            // start on the 18-half scale). Half tells who's batting: TOP (top
            // in progress) and END (bottom just finished, so the *next*
            // batter is the top of the following inning) both mean AWAY
            // bats; MID (top just finished, next batter is bottom of this
            // inning) and BOT (bottom in progress) both mean HOME bats.
            let inning_late = lateness(League::Mlb, &g.period, "") >= 77;
            let runners = bases.iter().filter(|b| **b).count() as u16;
            let (bat, field) = if g.period.starts_with("TOP") || g.period.starts_with("END") {
                (g.away_score, g.home_score)
            } else {
                (g.home_score, g.away_score)
            };
            if inning_late && runners > 0 && field > bat && field - bat <= runners + 1 {
                bonus!(40, Some("TYING RUN ON"));
            }
        }
        League::Nba | League::Wnba | League::Cbb => {
            // Only a confirmed clock reading under 2:00 counts — an
            // unparseable clock (None) never fires this bonus.
            let last2 = matches!(g.period.as_str(), "Q4" | "2ND HALF" | "OT")
                && clock_secs(&g.clock).is_some_and(|secs| secs > 0 && secs <= 120);
            if last2 && margin <= one_score(g.league) {
                bonus!(40, Some("CLUTCH"));
            }
        }
        League::Nhl => {
            if matches!(g.meter, Some(Meter::Penalty { .. })) {
                bonus!(25, Some("POWER PLAY"));
            }
        }
        League::Epl | League::Mls => {
            let stoppage = g.period.contains('+') || lateness(g.league, &g.period, "") >= 95;
            if stoppage && margin <= 1 {
                bonus!(30, Some("STOPPAGE"));
            }
        }
    }

    Watch { score, hot, chip }
}

/// How the board orders its live games. `Watch` is watchability (the default);
/// `Time` and `League` are the stable alternatives a viewer can cycle to when
/// they want a slate that never moves for reasons they can't see (spec §2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Watch,
    Time,
    League,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Watch => "WATCH",
            SortKey::Time => "TIME",
            SortKey::League => "LEAGUE",
        }
    }

    /// The next key in the WATCH → TIME → LEAGUE → WATCH cycle.
    pub fn cycled(self) -> SortKey {
        match self {
            SortKey::Watch => SortKey::Time,
            SortKey::Time => SortKey::League,
            SortKey::League => SortKey::Watch,
        }
    }
}

/// Render ticks a nudge stays visible: 10 s at the live cadence (spec §1).
pub const NUDGE_TICKS: u64 = 10 * crate::app::LIVE_TICKS_PER_SEC;

/// Owns the display order of live games. Reorders ONLY in `on_event`; between
/// events the order is frozen even as lateness rises (spec §2) — a board that
/// re-sorted on every render tick would slide out from under the eye.
#[derive(Default)]
pub struct OrderState {
    /// Game ids, in display order, as of the last event.
    order: Vec<String>,
    /// id -> (places risen, tick when it rose). Only risers are recorded.
    nudges: HashMap<String, (usize, u64)>,
}

/// Sort `games` by `key`, returning ids. Every comparison ends in the id so
/// the result is total — two games that tie on the key never swap between
/// events.
fn sorted_ids(games: &[Game], key: SortKey, now: OffsetDateTime) -> Vec<String> {
    let mut idx: Vec<&Game> = games.iter().collect();
    match key {
        SortKey::Watch => idx.sort_by(|a, b| {
            watchability(b, now)
                .score
                .cmp(&watchability(a, now).score)
                .then_with(|| a.id.cmp(&b.id))
        }),
        // A game with no start time sorts after every scheduled one (None >
        // Some for Option's own ordering would put it first, so map it to the
        // max explicitly).
        SortKey::Time => idx.sort_by(|a, b| {
            a.start
                .is_none()
                .cmp(&b.start.is_none())
                .then_with(|| a.start.cmp(&b.start))
                .then_with(|| a.id.cmp(&b.id))
        }),
        SortKey::League => idx.sort_by(|a, b| {
            league_pos(a.league)
                .cmp(&league_pos(b.league))
                .then_with(|| a.start.is_none().cmp(&b.start.is_none()))
                .then_with(|| a.start.cmp(&b.start))
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
    idx.into_iter().map(|g| g.id.clone()).collect()
}

/// Position in `League::ALL` — the canonical league order the tab row uses.
fn league_pos(l: League) -> usize {
    League::ALL
        .iter()
        .position(|x| *x == l)
        .unwrap_or(usize::MAX)
}

impl OrderState {
    /// Recompute after a data event. `games` = live games (pins excluded by the
    /// caller); returns nothing — read via `ordered`/`nudge`.
    pub fn on_event(&mut self, games: &[Game], key: SortKey, now: OffsetDateTime, tick: u64) {
        let next = sorted_ids(games, key, now);
        // The "before" picture is the old order with departed games removed:
        // a game going final must not read as a rise for everything under it.
        let before: Vec<&String> = self
            .order
            .iter()
            .filter(|id| next.iter().any(|n| n == *id))
            .collect();
        for (new_i, id) in next.iter().enumerate() {
            let Some(old_i) = before.iter().position(|o| *o == id) else {
                continue; // entering the board is not rising
            };
            if old_i > new_i {
                self.nudges.insert(id.clone(), (old_i - new_i, tick));
            }
        }
        self.nudges.retain(|id, _| next.iter().any(|n| n == id));
        self.order = next;
    }

    /// The stored order, resolved against `games`: ids the board no longer
    /// carries drop out, and games the last event never saw go at the end in
    /// board order (no key is stored, and sorting here would be a reorder
    /// between events — exactly what this type exists to prevent).
    pub fn ordered<'a>(&self, games: &'a [Game]) -> Vec<&'a Game> {
        let mut out: Vec<&'a Game> = self
            .order
            .iter()
            .filter_map(|id| games.iter().find(|g| g.id == *id))
            .collect();
        // Belt and braces: `on_event` should have seen every game already, so
        // this only catches a caller that read before the next event landed.
        out.extend(games.iter().filter(|g| !self.order.contains(&g.id)));
        out
    }

    /// `Some(n)` while game `id` shows an `↑n` nudge (10 s per spec §1).
    pub fn nudge(&self, id: &str, tick: u64) -> Option<usize> {
        self.nudges
            .get(id)
            .filter(|(_, t0)| tick.saturating_sub(*t0) < NUDGE_TICKS)
            .map(|(n, _)| *n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Situation, Team};
    use time::OffsetDateTime;

    fn g(league: League, period: &str, clock: &str, away: u16, home: u16) -> Game {
        Game {
            league,
            period: period.into(),
            clock: clock.into(),
            status: Status::Live,
            away_score: away,
            home_score: home,
            away: Team {
                abbr: "AAA".into(),
                ..Default::default()
            },
            home: Team {
                abbr: "HHH".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn now() -> OffsetDateTime {
        time::macros::datetime!(2026-09-13 16:47 -4)
    }

    #[test]
    fn lateness_reads_every_league_clock_grammar() {
        // Real strings from the nine leagues' fixtures.
        assert!(lateness(League::Nfl, "Q4", "1:52") > lateness(League::Nfl, "Q1", "12:00"));
        assert!(lateness(League::Mlb, "BOT 9TH", "") > lateness(League::Mlb, "TOP 2ND", ""));
        assert!(lateness(League::Epl, "78'", "") > lateness(League::Epl, "12'", ""));
        assert!(
            lateness(League::Epl, "90'+3'", "") >= 95,
            "stoppage is maximal"
        );
        assert!(
            lateness(League::Cbb, "2ND HALF", "3:10") > lateness(League::Cbb, "1ST HALF", "12:00")
        );
        assert!(lateness(League::Nhl, "3RD", "4:00") > lateness(League::Nhl, "1ST", "10:00"));
        assert_eq!(
            lateness(League::Nfl, "HALFTIME", ""),
            0,
            "unparsed period scores 0"
        );
        assert_eq!(lateness(League::Mlb, "", ""), 0);
    }

    #[test]
    fn closeness_uses_the_sports_own_scale() {
        // One-score football game is close; 17 points is not.
        assert!(closeness(League::Nfl, 3) > closeness(League::Nfl, 17));
        assert_eq!(closeness(League::Nfl, 0), 100);
        // A 1-run baseball game beats a 4-run one; hockey/soccer margins are goals.
        assert!(closeness(League::Mlb, 1) > closeness(League::Mlb, 4));
        assert!(closeness(League::Nhl, 1) > closeness(League::Nhl, 3));
        assert!(closeness(League::Nba, 5) > closeness(League::Nba, 18));
    }

    #[test]
    fn a_late_close_game_outranks_an_early_blowout_and_bonuses_make_hot() {
        let close_late = g(League::Nfl, "Q4", "1:52", 24, 21);
        let blowout_early = g(League::Nfl, "Q1", "12:00", 28, 0);
        assert!(watchability(&close_late, now()).score > watchability(&blowout_early, now()).score);
        // Red zone: hot + chip regardless of margin.
        let mut rz = g(League::Nfl, "Q3", "9:05", 31, 3);
        rz.meter = Some(Meter::RedZone { yards_to_goal: 4 });
        let w = watchability(&rz, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("RED ZONE"));
        // Two-minute window: Q4/Q2 under 2:00.
        let w2 = watchability(&g(League::Nfl, "Q4", "0:48", 17, 17), now());
        assert!(w2.hot);
        assert_eq!(w2.chip, Some("2-MIN"));
    }

    #[test]
    fn baseball_bonuses_bases_loaded_and_tying_run_late() {
        let mut bl = g(League::Mlb, "BOT 4TH", "", 2, 3);
        bl.situation = Some(Situation {
            on_base: Some([true, true, true]),
            outs: Some(1),
            balls: Some(3),
            strikes: Some(2),
            ..Default::default()
        });
        let w = watchability(&bl, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("BASES LOADED"));
        // Tying run on base in the 8th+ (batting team down 1, runner on 3rd).
        let mut ty = g(League::Mlb, "BOT 9TH", "", 8, 7); // home batting, down 1
        ty.situation = Some(Situation {
            on_base: Some([false, false, true]),
            outs: Some(2),
            balls: Some(1),
            strikes: Some(0),
            ..Default::default()
        });
        let w = watchability(&ty, now());
        assert!(w.hot);
        assert_eq!(
            w.chip,
            Some("TYING RUN ON"),
            "chip prefix; the base is appended by the caller"
        );
    }

    #[test]
    fn mlb_end_half_batting_side_is_away_not_home() {
        // END 8TH: bottom just finished, next batter is the top of the 9th
        // -> AWAY bats. Away trails by 1 with a runner on 2nd -> tying run
        // on, hot.
        let mut end_away_down = g(League::Mlb, "END 8TH", "", 5, 6); // away 5, home 6
        end_away_down.situation = Some(Situation {
            on_base: Some([false, true, false]),
            outs: Some(1),
            balls: Some(2),
            strikes: Some(1),
            ..Default::default()
        });
        let w = watchability(&end_away_down, now());
        assert!(w.hot, "away is the batting/tying side on END, must be hot");
        assert_eq!(w.chip, Some("TYING RUN ON"));

        // Same shape relabeled BOT 8TH: bottom in progress, HOME bats. Home
        // trails by 1 with a runner on 2nd -> tying run on, hot.
        let mut bot_home_down = g(League::Mlb, "BOT 8TH", "", 6, 5); // away 6, home 5
        bot_home_down.situation = Some(Situation {
            on_base: Some([false, true, false]),
            outs: Some(1),
            balls: Some(2),
            strikes: Some(1),
            ..Default::default()
        });
        let w = watchability(&bot_home_down, now());
        assert!(w.hot, "home is the batting/tying side on BOT, must be hot");
        assert_eq!(w.chip, Some("TYING RUN ON"));

        // END 8TH again, but HOME trails (so AWAY, the batting side, is
        // actually ahead) — the old `starts_with("TOP")`-vs-else code
        // wrongly treated HOME as batting here and fired hot; the batting
        // side (away) is not the trailing team, so this must NOT be hot.
        let mut end_home_down = g(League::Mlb, "END 8TH", "", 6, 5); // away 6, home 5
        end_home_down.situation = Some(Situation {
            on_base: Some([false, true, false]),
            outs: Some(1),
            balls: Some(2),
            strikes: Some(1),
            ..Default::default()
        });
        let w = watchability(&end_home_down, now());
        assert!(
            !w.hot,
            "away bats next on END and is already ahead, not tying"
        );
    }

    #[test]
    fn junk_clock_reads_as_segment_start_not_fully_elapsed() {
        assert_eq!(
            lateness(League::Nfl, "Q4", "junk"),
            lateness(League::Nfl, "Q4", "15:00"),
            "unparseable clock counts as 0 elapsed in the segment"
        );
        assert!(lateness(League::Nfl, "Q4", "junk") < lateness(League::Nfl, "Q4", "1:52"));

        // A junk clock must never falsely trigger the 2-MIN bonus.
        let w = watchability(&g(League::Nfl, "Q4", "junk", 17, 17), now());
        assert_ne!(w.chip, Some("2-MIN"));
    }

    #[test]
    fn non_live_games_score_zero_and_never_chip() {
        let mut f = g(League::Nfl, "Q4", "", 24, 21);
        f.status = Status::Final;
        assert_eq!(
            watchability(&f, now()),
            Watch {
                score: 0,
                hot: false,
                chip: None
            }
        );
        let mut p = g(League::Nfl, "", "", 0, 0);
        p.status = Status::Pre;
        assert_eq!(watchability(&p, now()).score, 0);
    }

    /// Ids in display order. A helper because `ordered` borrows the slice it
    /// is given, so the games have to outlive the call.
    fn ids(os: &OrderState, games: &[Game]) -> Vec<String> {
        os.ordered(games).iter().map(|g| g.id.clone()).collect()
    }

    #[test]
    fn order_is_frozen_between_events_and_nudges_mark_risers() {
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q2", "8:00", 14, 10);
        a.id = "a".into();
        let mut b = g(League::Nfl, "Q4", "1:52", 24, 21);
        b.id = "b".into();
        let mut c = g(League::Mlb, "TOP 3RD", "", 1, 0);
        c.id = "c".into();
        let board = [a.clone(), b.clone(), c.clone()];
        os.on_event(&board, SortKey::Watch, now(), 0);
        let ids1 = ids(&os, &board);
        assert_eq!(ids1[0], "b", "late close game leads");
        // No event: calling ordered again (later clock would rank differently) keeps order.
        let mut a2 = a.clone();
        a2.period = "Q4".into();
        a2.clock = "0:30".into();
        let later = [a2.clone(), b.clone(), c.clone()];
        assert_eq!(ids1, ids(&os, &later), "no event, no reorder");
        // Event: a's score changes; it rises and carries a nudge.
        let mut a3 = a2.clone();
        a3.away_score = 24;
        a3.home_score = 24;
        let scored = [a3.clone(), b.clone(), c.clone()];
        os.on_event(&scored, SortKey::Watch, now(), 100);
        assert_eq!(
            ids(&os, &scored)[0],
            "a",
            "tied in the last minute now leads"
        );
        assert_eq!(os.nudge("a", 100), Some(1), "rose one place");
        assert_eq!(os.nudge("a", 100 + NUDGE_TICKS), None, "nudge expires");
        assert_eq!(os.nudge("b", 100), None, "the faller gets nothing");
    }

    #[test]
    fn sort_keys_time_and_league_are_stable_alternatives() {
        let mut os = OrderState::default();
        let mut a = g(League::Mlb, "TOP 1ST", "", 0, 0);
        a.id = "a".into();
        a.start = Some(time::macros::datetime!(2026-09-13 13:05 -4));
        let mut b = g(League::Nfl, "Q1", "15:00", 0, 0);
        b.id = "b".into();
        b.start = Some(time::macros::datetime!(2026-09-13 13:00 -4));
        let board = [a.clone(), b.clone()];
        os.on_event(&board, SortKey::Time, now(), 0);
        assert_eq!(ids(&os, &board), vec!["b", "a"], "earlier start first");
        os.on_event(&board, SortKey::League, now(), 0);
        assert_eq!(
            ids(&os, &board),
            vec!["b", "a"],
            "NFL precedes MLB in League::ALL order"
        );
    }

    #[test]
    fn a_new_game_joins_without_scrambling_the_rest() {
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q4", "1:00", 20, 17);
        a.id = "a".into();
        os.on_event(&[a.clone()], SortKey::Watch, now(), 0);
        let mut b = g(League::Nfl, "Q1", "15:00", 0, 0);
        b.id = "b".into();
        // b entering IS an event.
        let board = [a.clone(), b.clone()];
        os.on_event(&board, SortKey::Watch, now(), 10);
        assert_eq!(ids(&os, &board), vec!["a", "b"]);
        assert_eq!(os.nudge("b", 10), None, "entering is not rising");
    }

    #[test]
    fn soccer_stoppage_within_one_goal_is_hot() {
        let w = watchability(&g(League::Epl, "90'+2'", "", 1, 1), now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("STOPPAGE"));
        let w2 = watchability(&g(League::Epl, "90'+2'", "", 4, 0), now());
        assert!(!w2.hot, "a stoppage blowout is not hot");
    }
}
