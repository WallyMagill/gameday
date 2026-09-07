//! Watchability: which game deserves the hero and the hot mark. Pure
//! functions of the Game — the clock grammar strings come straight from the
//! mapper (fixtures verified per league), and an unparsed string scores 0 so
//! bad data can never lead the board.
use crate::domain::{Extras, Game, League, Meter, Status};
use std::collections::HashMap;
use time::OffsetDateTime;

/// One game's watchability verdict. `score` orders the board; `hot` drives
/// the 2-state mark; `chip` is the hero/state label ("RED ZONE", "2-MIN",
/// "TYING RUN 3RD", "BASES LOADED", "STOPPAGE", …) or None.
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
            // inflate lateness.
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
/// rather than silently reading it as fully elapsed.
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
            // Red zone bonus. The source is ESPN's own
            // `situation.isRedZone`, not a yard number re-derived from
            // `possessionText`. An absent flag is not a "no" but it is not
            // a chip either — the board stays quiet rather than guessing.
            // Also requires a possessing team: the flag alone doesn't say
            // which goal is threatened (the mapper already withholds the
            // meter for the same reason), so no possession earns no chip.
            if g.situation
                .as_ref()
                .is_some_and(|s| s.is_red_zone == Some(true) && s.possession.is_some())
            {
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
            // `field >= bat`: a tied game with a runner on is the walk-off
            // situation, and the rule is "tying/**go-ahead** run on base" —
            // the old `>` gave the tied 9th no chip and no hot mark. At 0
            // margin the runner is the go-ahead run, so the chip says so.
            if inning_late && runners > 0 && field >= bat && field - bat <= runners + 1 {
                // Name the base — the lead runner's, the one whose run ties or
                // wins it. The chip is a `&'static str`, so these are the six
                // spellings, not a `format!`. Thirteen cells is the budget rather
                // than a style choice: the tier-1 chip rides under the clock in
                // `rows::T1_CHIP_W`, 13 cells to the play-text column, so the
                // literal "TYING RUN ON 3RD" (16) would still paint into the play
                // — "TYING RUN 3RD" (13) fits exactly.
                let lead = bases.iter().rposition(|b| *b).unwrap_or(0);
                let chip = match (field == bat, lead) {
                    (true, 0) => "GO-AHEAD 1ST",
                    (true, 1) => "GO-AHEAD 2ND",
                    (true, _) => "GO-AHEAD 3RD",
                    (false, 0) => "TYING RUN 1ST",
                    (false, 1) => "TYING RUN 2ND",
                    (false, _) => "TYING RUN 3RD",
                };
                bonus!(40, Some(chip));
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
            // Real NHL strength is summary-derived, and the summary
            // lands only for the game the viewer has zoomed. Scoring it
            // would put a POWER PLAY chip and the hot flag on that one board
            // row while an identical unzoomed power play three rows down
            // wore nothing — a board that changes with where the viewer has
            // been looking, and an ordering bonus only the zoomed game could
            // earn. So no real penalty meter is ever stored on a `Game`
            // (`Extras::penalty_meter` is derived at the zoom instead), and
            // this arm refuses one anyway if a future change puts it there:
            // `Extras::Hockey` is the receipt that the data came from a
            // summary, built by the same mapper in the same place.
            //
            // What is left firing is demo and sim data (`demo.rs`,
            // `sim.rs`), which set a penalty meter with no `Extras::Hockey`
            // — so the gallery keeps its showcase. Real NHL strength reaches
            // the board when the October scoreboard probe says which field
            // carries it; promotion is additive and this arm is where it
            // lands.
            let summary_derived = matches!(g.extras, Extras::Hockey { .. });
            if !summary_derived && matches!(g.meter, Some(Meter::Penalty { .. })) {
                bonus!(25, Some("POWER PLAY"));
            }
        }
        League::Epl | League::Mls => {
            // A sending-off, first. It outranks STOPPAGE for
            // the chip because it is the state that reshapes everything left
            // of the match, while stoppage time is the minute you are in.
            //
            // Value 40 — the CLUTCH class (RED ZONE 40, CLUTCH 40, the MLB
            // tying/go-ahead run 40), not the POWER PLAY class. The receipt
            // is duration: a power play (25) is a two-minute advantage that
            // expires on its own, and STOPPAGE (30) is a handful of minutes;
            // a red card is permanent — a side plays a man down for every
            // minute remaining, exactly the "watch this one now" the top
            // tier is for. It is also the only one of the four that cannot
            // un-happen.
            //
            // The chip is a `&'static str` like every other, so these are
            // the spellings rather than a `format!`. Seven cells at worst
            // ("11 MEN" is unreachable), well inside `rows::T1_CHIP_W`'s 13.
            // Below eight men the match is abandoned, so the ladder stops
            // there and anything lower reads as the floor rather than
            // inventing a word for a scoreline that cannot exist.
            if let Extras::Soccer {
                men: Some((a, h)), ..
            } = &g.extras
            {
                let chip = match (*a).min(*h) {
                    10 => Some("10 MEN"),
                    9 => Some("9 MEN"),
                    _ => Some("8 MEN"),
                };
                bonus!(40, chip);
            }
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
/// they want a slate that never moves for reasons they can't see.
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

/// Render ticks a nudge stays visible: 10 s at the live cadence.
pub const NUDGE_TICKS: u64 = 10 * crate::app::LIVE_TICKS_PER_SEC;

/// Owns the display order of live games. Reorders ONLY in `on_event`; between
/// events the order is frozen even as lateness rises — a board that
/// re-sorted on every render tick would slide out from under the eye.
#[derive(Default)]
pub struct OrderState {
    /// Game ids, in display order, as of the last event.
    order: Vec<String>,
    /// id -> (places risen, tick when it rose). Only risers are recorded.
    nudges: HashMap<String, (usize, u64)>,
}

/// Sort `games` by `key`, returning ids. `enabled` is the viewer's tab order —
/// the League key follows it, not `League::ALL`, so the board reads in the
/// order the tab row shows. Every comparison ends in the id so the
/// result is total: two games that tie on the key never swap between events.
fn sorted_ids(
    games: &[Game],
    key: SortKey,
    enabled: &[League],
    now: OffsetDateTime,
) -> Vec<String> {
    let mut idx: Vec<&Game> = games.iter().collect();
    match key {
        // `sort_by_cached_key` so `watchability` runs once per game, not once
        // per comparison. Reverse the score (u32::MAX - s) to get descending
        // out of an ascending sort, then the id for a total order.
        SortKey::Watch => {
            idx.sort_by_cached_key(|g| (u32::MAX - watchability(g, now).score, g.id.clone()))
        }
        // `is_none()` leads because `None < Some(_)` in Option's own ordering,
        // which would float an unscheduled game to the top; false sorts before
        // true, so this puts it last instead.
        SortKey::Time => idx.sort_by_cached_key(|g| {
            (
                g.start.is_none(),
                g.start,
                league_pos(g.league, enabled),
                g.id.clone(),
            )
        }),
        SortKey::League => idx.sort_by_cached_key(|g| {
            (
                league_pos(g.league, enabled),
                g.start.is_none(),
                g.start,
                g.id.clone(),
            )
        }),
    }
    idx.into_iter().map(|g| g.id.clone()).collect()
}

/// Who the ranking would put first RIGHT NOW, ignoring the frozen display
/// order. `:tv` is the only caller: the screen switches on the next event,
/// so between events it has to be able to name the game it will
/// switch to without moving there. Nothing here reorders anything.
pub fn top_id(
    games: &[Game],
    key: SortKey,
    enabled: &[League],
    now: OffsetDateTime,
) -> Option<String> {
    sorted_ids(games, key, enabled, now).into_iter().next()
}

/// Position in the viewer's enabled-tab order. A league that is not enabled
/// (a pinned game's league, say) sorts after every enabled one.
fn league_pos(l: League, enabled: &[League]) -> usize {
    enabled.iter().position(|x| *x == l).unwrap_or(usize::MAX)
}

impl OrderState {
    /// Recompute after a data event. `games` = live games (pins excluded by the
    /// caller), `enabled` = the viewer's tab order; returns nothing — read via
    /// `ordered`/`nudge`.
    pub fn on_event(
        &mut self,
        games: &[Game],
        key: SortKey,
        enabled: &[League],
        now: OffsetDateTime,
        tick: u64,
    ) {
        let next = sorted_ids(games, key, enabled, now);
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
            } else {
                // Holding still or falling clears any live arrow: a row that
                // dropped must not keep showing the ↑n it earned a moment ago.
                self.nudges.remove(id);
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

    /// `Some(n)` while game `id` shows an `↑n` nudge (10 s at the live cadence).
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
        // Red zone: hot + chip regardless of margin. Source is ESPN's own
        // `isRedZone`, not the meter the gauge draws.
        let mut rz = g(League::Nfl, "Q3", "9:05", 31, 3);
        rz.situation = Some(Situation {
            is_red_zone: Some(true),
            possession: Some("KC".into()),
            ..Default::default()
        });
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
    fn a_red_zone_flag_without_possession_earns_no_chip() {
        let mut g = g(League::Nfl, "Q3", "9:05", 21, 17);
        g.situation = Some(Situation {
            is_red_zone: Some(true),
            possession: None,
            ..Default::default()
        });
        let w = watchability(&g, OffsetDateTime::now_utc());
        assert_eq!(w.chip, None);
        assert!(!w.hot);
    }

    /// The RED ZONE chip is ESPN's `situation.isRedZone`, not a
    /// re-derivation from `possessionText`. The old parse split the text on
    /// its last space and read the tail as a yard number, so a text it
    /// couldn't split lost the chip and a text ending in a small number won
    /// one — both are now impossible, because the text is never consulted.
    #[test]
    fn the_red_zone_chip_reads_the_payload_not_the_text() {
        // Text the old rsplit parse would have failed on ("3" is there, but
        // "weird &format" was never a team abbreviation) — the flag decides.
        let mut fires = g(League::Nfl, "Q3", "9:05", 31, 3);
        fires.situation = Some(Situation {
            possession: Some("KC".into()),
            ball_on: Some("weird &format 3".into()),
            is_red_zone: Some(true),
            ..Default::default()
        });
        let w = watchability(&fires, now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("RED ZONE"));

        // Text the old parse would have been TRICKED by ("TB 3" reads as the
        // opponent's 3-yard line) while ESPN says the ball is not in the red
        // zone. No chip.
        let mut quiet = g(League::Nfl, "Q3", "9:05", 31, 3);
        quiet.situation = Some(Situation {
            possession: Some("KC".into()),
            ball_on: Some("TB 3".into()),
            is_red_zone: Some(false),
            ..Default::default()
        });
        assert_eq!(watchability(&quiet, now()).chip, None);

        // Absent flag (non-live, non-football, or a feed that just doesn't
        // send it): the chip does not fire from the situation at all.
        let mut silent = g(League::Nfl, "Q3", "9:05", 31, 3);
        silent.situation = Some(Situation {
            ball_on: Some("TB 3".into()),
            ..Default::default()
        });
        let w = watchability(&silent, now());
        assert_eq!(w.chip, None);
        assert!(!w.hot, "no flag, no red-zone heat");
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
            Some("TYING RUN 3RD"),
            "the chip must name the base the tying run stands on — a bare \
             \"TYING RUN\" is a dangling phrase that leaves the reader asking \
             which base, and the base is the whole reason the situation is hot"
        );
    }

    #[test]
    fn mlb_tied_ninth_with_a_runner_is_the_go_ahead_situation() {
        // BOT 9TH tied, winning run on third: the walk-off. The old
        // `field > bat` gate gave this no bonus and no chip at all.
        let mut walkoff = g(League::Mlb, "BOT 9TH", "", 4, 4);
        walkoff.situation = Some(Situation {
            on_base: Some([false, false, true]),
            outs: Some(1),
            ..Default::default()
        });
        let w = watchability(&walkoff, now());
        assert!(w.hot, "a tied 9th with the winning run on third is hot");
        assert_eq!(w.chip, Some("GO-AHEAD 3RD"));

        // Tied but nobody on: no runner, no chip.
        let mut empty = walkoff.clone();
        empty.situation = Some(Situation {
            on_base: Some([false, false, false]),
            outs: Some(1),
            ..Default::default()
        });
        assert_eq!(watchability(&empty, now()).chip, None);

        // The lead runner names the base: first and second occupied, tied,
        // so the go-ahead run is the one on second.
        let mut first_and_second = walkoff.clone();
        first_and_second.situation = Some(Situation {
            on_base: Some([true, true, false]),
            outs: Some(1),
            ..Default::default()
        });
        assert_eq!(
            watchability(&first_and_second, now()).chip,
            Some("GO-AHEAD 2ND")
        );

        // Down 1 with only a runner on first: he is the tying run.
        let mut down_one = g(League::Mlb, "BOT 9TH", "", 5, 4);
        down_one.situation = Some(Situation {
            on_base: Some([true, false, false]),
            outs: Some(1),
            ..Default::default()
        });
        assert_eq!(
            watchability(&down_one, now()).chip,
            Some("TYING RUN 1ST") // and the chip names the base he stands on
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
        // The chip is named for the runner's base, so END and BOT read alike.
        assert_eq!(w.chip, Some("TYING RUN 2ND"));

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
        // Same chip: the half changed, the base the runner stands on did not.
        assert_eq!(w.chip, Some("TYING RUN 2ND"));

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

    /// Every league enabled, in the canonical tab order — what the League key
    /// follows unless a test says otherwise.
    fn all() -> Vec<League> {
        League::ALL.to_vec()
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
        os.on_event(&board, SortKey::Watch, &all(), now(), 0);
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
        os.on_event(&scored, SortKey::Watch, &all(), now(), 100);
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
    fn a_row_that_falls_back_loses_its_arrow_inside_the_window() {
        // Two events inside NUDGE_TICKS: a rises, then a falls back. The
        // arrow it earned must be gone, not linger for the rest of the 10 s.
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q4", "8:00", 14, 10);
        a.id = "a".into();
        let mut b = g(League::Nfl, "Q4", "8:00", 24, 21);
        b.id = "b".into();
        os.on_event(&[a.clone(), b.clone()], SortKey::Watch, &all(), now(), 0);
        assert_eq!(ids(&os, &[a.clone(), b.clone()]), vec!["b", "a"]);
        // a ties it up in the last minute and leads.
        let mut a_up = a.clone();
        a_up.clock = "0:30".into();
        a_up.away_score = 21;
        a_up.home_score = 21;
        os.on_event(
            &[a_up.clone(), b.clone()],
            SortKey::Watch,
            &all(),
            now(),
            10,
        );
        assert_eq!(os.nudge("a", 10), Some(1), "a rose");
        // Within the same window b ties its own game up while a's opponent
        // pulls away: b takes the lead back and a drops.
        let mut b_up = b.clone();
        b_up.clock = "0:20".into();
        b_up.away_score = 24;
        b_up.home_score = 24;
        let mut a_down = a_up.clone();
        a_down.away_score = 28; // a is a one-score-plus game again
        os.on_event(
            &[a_down.clone(), b_up.clone()],
            SortKey::Watch,
            &all(),
            now(),
            20,
        );
        assert_eq!(ids(&os, &[a_down.clone(), b_up.clone()]), vec!["b", "a"]);
        assert_eq!(os.nudge("b", 20), Some(1), "b rose back");
        assert_eq!(
            os.nudge("a", 20),
            None,
            "a fell inside the window; its arrow is cleared, not left to expire"
        );
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
        os.on_event(&board, SortKey::Time, &all(), now(), 0);
        assert_eq!(ids(&os, &board), vec!["b", "a"], "earlier start first");
        os.on_event(&board, SortKey::League, &all(), now(), 0);
        assert_eq!(
            ids(&os, &board),
            vec!["b", "a"],
            "NFL precedes MLB in the default tab order"
        );
        // The League key follows the viewer's tab order, so moving MLB
        // to the front of `enabled_tabs` moves it to the front of the board.
        let mlb_first = [League::Mlb, League::Nfl];
        os.on_event(&board, SortKey::League, &mlb_first, now(), 0);
        assert_eq!(
            ids(&os, &board),
            vec!["a", "b"],
            "League follows enabled_tabs, not League::ALL"
        );
    }

    #[test]
    fn a_new_game_joins_without_scrambling_the_rest() {
        let mut os = OrderState::default();
        let mut a = g(League::Nfl, "Q4", "1:00", 20, 17);
        a.id = "a".into();
        os.on_event(&[a.clone()], SortKey::Watch, &all(), now(), 0);
        let mut b = g(League::Nfl, "Q1", "15:00", 0, 0);
        b.id = "b".into();
        // b entering IS an event.
        let board = [a.clone(), b.clone()];
        os.on_event(&board, SortKey::Watch, &all(), now(), 10);
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

    /// A sending-off is hot at any scoreline, names the count,
    /// and outranks STOPPAGE for the chip.
    #[test]
    fn a_sending_off_is_hot_at_any_scoreline() {
        let carded = |men: Option<(u8, u8)>, period: &str, away, home| {
            let mut x = g(League::Epl, period, "", away, home);
            x.extras = Extras::Soccer {
                events: vec![],
                men,
            };
            x
        };

        // Even a 4-0 rout at 63' — a scoreline with a closeness of 0 — is hot
        // once a side is down to ten.
        let w = watchability(&carded(Some((10, 11)), "63'", 4, 0), now());
        assert!(w.hot);
        assert_eq!(w.chip, Some("10 MEN"));
        assert_eq!(w.score, 40, "the bonus is the CLUTCH tier, on a base of 0");

        // The chip names the SHORT side's count, whichever side that is.
        assert_eq!(
            watchability(&carded(Some((11, 9)), "63'", 1, 1), now()).chip,
            Some("9 MEN")
        );
        // Nine-a-side floors at the last spelling rather than inventing one.
        assert_eq!(
            watchability(&carded(Some((7, 11)), "63'", 1, 1), now()).chip,
            Some("8 MEN")
        );

        // Stoppage time on top adds its bonus but not its chip: the card is
        // the state that shapes what is left of the match.
        let both = watchability(&carded(Some((10, 11)), "90'+2'", 1, 1), now());
        assert_eq!(both.chip, Some("10 MEN"));
        // 95 = the stoppage lateness floor at a closeness of 100.
        assert_eq!(both.score, 95 + 40 + 30, "both bonuses still score");

        // Eleven a side says nothing at all.
        let quiet = watchability(&carded(None, "63'", 4, 0), now());
        assert!(!quiet.hot);
        assert_eq!(quiet.chip, None);
    }
}
