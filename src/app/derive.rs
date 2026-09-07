//! The derived game lists — every list a frame renders, and the `Derived`
//! bundle that evaluates them once per draw instead of once per widget.
//!
//! This is the single source of truth: the board is ONE ranked list
//! cut into sections, and `derive()` is the only place those sections are
//! decided. The standalone `live_games`/`slate_games`/`mosaic_games`/
//! `selection_list` methods are gone with the tile grammar — the app once kept
//! both a set of list fns and a `Derived` built out of them, and a test whose
//! whole job was to prove the two never drift. Key handlers that need a list
//! outside a draw call `derive()` (cheap off-frame, once per keypress);
//! `App::draw` parks one in `frame_cache` for the widgets to read through
//! `derived()`.

use super::{App, Tab};
use crate::config::prune_pins;
use crate::domain::{Game, League, Play, Status};
use crate::home::home_games;
use std::cell::Cell;

thread_local! {
    /// How many times [`App::derive`] has run on this thread. Only the
    /// once-per-frame test reads it; the counter itself is a `Cell` bump,
    /// too cheap to be worth compiling out.
    pub(crate) static DERIVE_COUNT: Cell<u32> = const { Cell::new(0) };
}

/// Every game list one frame renders, evaluated together — the board's four
/// sections, the selection they concatenate into, and the feeds the
/// chrome reads.
pub struct Derived {
    /// The MY GAMES band: pins first (pin order), then favorited-team games
    /// (board order), deduped — a pinned favorite appears once. Never
    /// re-sorted.
    pub my_games: Vec<Game>,
    /// Live games that are NOT in the band, in `OrderState`'s frozen order.
    pub in_play: Vec<Game>,
    pub finals: Vec<Game>,
    pub later: Vec<Game>,
    /// What j/k walks: `my_games ++ in_play ++ finals ++ later`, which is
    /// exactly the order the board draws.
    pub selection: Vec<Game>,
    /// The hero game: the top of MY GAMES when that game is live, else the
    /// best live game, else the first thing on the board.
    pub hero_id: Option<String>,
    /// More than one league on the board — the rows print league tags only
    /// then (A′ call #5).
    pub mixed: bool,
    pub scoring: Vec<(Game, Play)>,
    pub ticker_live: Vec<Game>,
    pub ticker_events: Vec<(Game, Play)>,
}

impl App {
    pub fn visible_games(&self) -> Vec<Game> {
        let games = match self.tab {
            Tab::Home => {
                let concat = self.concat_boards();
                home_games(&self.pins, &self.config.favorites, &concat, self.now())
                    .into_iter()
                    .cloned()
                    .collect()
            }
            Tab::League(league) => self.league_games(league),
        };
        let q = self.active_filter().map(crate::filter::Query::parse);
        games
            .into_iter()
            .filter(|g| q.as_ref().is_none_or(|q| q.matches(g)))
            .collect()
    }

    /// One league's board as the tab shows it: today's live board, or — while
    /// date-traveled — the fetched slate for the viewed date (empty until the
    /// on-demand fetch answers).
    fn league_games(&self, league: League) -> Vec<Game> {
        match self.viewed_date(league) {
            Some(date) => self
                .dated_boards
                .get(&(league, date))
                .cloned()
                .unwrap_or_default(),
            None => self.boards.get(&league).cloned().unwrap_or_default(),
        }
    }

    pub(crate) fn concat_boards(&self) -> Vec<Game> {
        let mut out = Vec::new();
        for league in &self.config.enabled_tabs {
            if let Some(games) = self.boards.get(league) {
                out.extend(games.iter().cloned());
            }
        }
        out
    }

    /// Is this game one of the viewer's? Pins and favorites both: the MY GAMES
    /// band holds both, and both are kept out of IN PLAY,
    /// which is why `live_all` — what `OrderState` ranks — asks this too.
    pub(crate) fn is_my_game(&self, game: &Game) -> bool {
        self.pins.iter().any(|p| p.game_id == game.id) || self.favorited(game)
    }

    pub(crate) fn favorited(&self, game: &Game) -> bool {
        self.config.favorites.iter().any(|fav| {
            fav.league == game.league
                && (game.away.abbr.eq_ignore_ascii_case(&fav.team_abbr)
                    || game.home.abbr.eq_ignore_ascii_case(&fav.team_abbr))
        })
    }

    /// How many rows j/k can land on, without building the lists. The four
    /// sections partition `visible_games()` — every visible game lands in
    /// exactly one of them — so the selection is the same length as the
    /// visible board. `clamp_selected`/`move_selected` run on the keypress
    /// path and only ever wanted this number; a full `derive()` there also
    /// cloned the whole scoring feed. The invariant is asserted in
    /// `the_sections_partition_the_board_in_selection_order`.
    pub(crate) fn selection_len(&self) -> usize {
        self.visible_games().len()
    }

    pub(crate) fn selected_game(&self) -> Option<Game> {
        self.derive().selection.get(self.selected).cloned()
    }

    /// Scoring plays across every enabled board, newest first per game,
    /// games in board order. Finals keep theirs until they leave the board.
    pub fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let mut out = Vec::new();
        for game in self.concat_boards() {
            for play in game.scoring_plays.iter().rev() {
                out.push((game.clone(), play.clone()));
            }
        }
        out
    }

    /// What an empty filtered board says. A filter that misses is almost
    /// always a scope mistake — the team is playing, just not on this tab —
    /// so the message names the scope it searched and, when the filter does
    /// match somewhere live, the games it found. Up to two: a list long
    /// enough to wrap stops being a hint.
    pub(crate) fn filter_miss_message(&self) -> String {
        let needle = self.active_filter().unwrap_or_default().to_string();
        let scope = match self.tab {
            Tab::Home => "all boards".to_string(),
            Tab::League(league) => league.slug().to_uppercase(),
        };
        let matches: Vec<String> = self
            .ticker_live()
            .iter()
            .take(2)
            .map(|g| format!("{}@{}", g.away.abbr, g.home.abbr))
            .collect();
        let elsewhere = if matches.is_empty() {
            String::new()
        } else {
            format!(" · ticker matches {}", matches.join(", "))
        };
        format!("no games match \"{needle}\" on {scope}{elsewhere} · esc clears")
    }

    /// The empty board's one line for a league tab: the day it is showing
    /// and the first game after that day the app knows about — today's
    /// board plus every slate `[`/`]` fetched — or `nothing scheduled`
    /// when none is loaded (the claim covers the loaded window; `]` fetches
    /// the next day on demand).
    pub(crate) fn empty_league_line(&self, league: League) -> String {
        let now = self.now();
        let date = self.viewed_date(league).unwrap_or(now.date());
        let next = self
            .boards
            .get(&league)
            .into_iter()
            .flatten()
            .chain(
                self.dated_boards
                    .iter()
                    .filter(|((l, _), _)| *l == league)
                    .flat_map(|(_, games)| games.iter()),
            )
            .filter(|g| g.status == Status::Pre)
            .filter_map(|g| g.start.map(|s| (s.to_offset(now.offset()), g)))
            .filter(|(s, _)| s.date() > date)
            .min_by_key(|(s, _)| *s);
        let head = format!(
            "No {} games {}",
            league.slug().to_uppercase(),
            super::date_label(date)
        );
        match next {
            Some((start, g)) => format!(
                "{head} · next {} {} @ {}",
                crate::text::fmt_start(start, now),
                g.away.abbr,
                g.home.abbr
            ),
            None => format!("{head} · nothing scheduled"),
        }
    }

    /// Every live game across the enabled boards, league order — the ticker
    /// covers what the visible tab (or a traveled date) does not. A typed
    /// filter is explicit intent, so it narrows the ticker too.
    pub(crate) fn ticker_live(&self) -> Vec<Game> {
        let q = self.active_filter().map(crate::filter::Query::parse);
        self.concat_boards()
            .into_iter()
            .filter(|g| g.status == Status::Live)
            .filter(|g| q.as_ref().is_none_or(|q| q.matches(g)))
            .collect()
    }

    /// The board's four sections, built from one walk of `visible_games`.
    /// This is the spec: the band is pins-then-favorites in that fixed order,
    /// IN PLAY is whatever `OrderState` last froze, and FINAL/LATER are the
    /// leftovers by status. `selection` is their concatenation — the same
    /// order the board draws, so j/k and the rows can never disagree.
    pub(crate) fn derive(&self) -> Derived {
        DERIVE_COUNT.with(|c| c.set(c.get() + 1));
        let visible = self.visible_games();

        // The band: pruned pins in pin order, then favorites in board order.
        let mut my_games: Vec<Game> = Vec::new();
        for pin in prune_pins(self.pins.clone(), self.now()) {
            if let Some(g) = visible.iter().find(|g| g.id == pin.game_id) {
                if !my_games.iter().any(|x| x.id == g.id) {
                    my_games.push(g.clone());
                }
            }
        }
        for g in &visible {
            if self.favorited(g) && !my_games.iter().any(|x| x.id == g.id) {
                my_games.push(g.clone());
            }
        }

        let rest: Vec<&Game> = visible
            .iter()
            .filter(|g| !my_games.iter().any(|m| m.id == g.id))
            .collect();
        let live: Vec<Game> = rest
            .iter()
            .filter(|g| g.status == Status::Live)
            .map(|g| (*g).clone())
            .collect();
        let in_play: Vec<Game> = self.order.ordered(&live).into_iter().cloned().collect();
        let finals: Vec<Game> = rest
            .iter()
            .filter(|g| g.status == Status::Final)
            .map(|g| (*g).clone())
            .collect();
        let later: Vec<Game> = rest
            .iter()
            .filter(|g| g.status == Status::Pre)
            .map(|g| (*g).clone())
            .collect();

        let selection: Vec<Game> = my_games
            .iter()
            .chain(in_play.iter())
            .chain(finals.iter())
            .chain(later.iter())
            .cloned()
            .collect();

        // The hero is the top of MY GAMES when it is live (a pin
        // outranks watchability), else the best live game. With nothing live
        // at all there is no hero: a third fallback to `selection.first()`
        // used to reach for a FINAL/LATER
        // game, but `board/mod.rs` only ever draws a `Hero` block for a game
        // in the band or in `in_play` — that game would render as a plain
        // tier-3 row regardless, so the fallback never actually put a
        // headline on screen. Dropped rather than wired through, since a
        // board with nothing live genuinely has nothing to feature.
        let hero_id = my_games
            .first()
            .filter(|g| g.status == Status::Live)
            .or_else(|| in_play.first())
            .map(|g| g.id.clone());

        let mut leagues: Vec<League> = Vec::new();
        for g in &visible {
            if !leagues.contains(&g.league) {
                leagues.push(g.league);
            }
        }
        let mixed = leagues.len() > 1;

        let scoring = self.scoring_events();
        let ticker_live = self.ticker_live();
        // Scoring plays of the ticker's live games, board order.
        let ticker_events = scoring
            .iter()
            .filter(|(g, _)| ticker_live.iter().any(|l| l.id == g.id))
            .cloned()
            .collect();
        Derived {
            my_games,
            in_play,
            finals,
            later,
            selection,
            hero_id,
            mixed,
            scoring,
            ticker_live,
            ticker_events,
        }
    }

    /// This frame's lists. Valid only inside `App::draw`, which fills the
    /// cache on its first line and drops it on its last — outside a draw
    /// there is no frame to be derived for, so this panics rather than
    /// silently deriving a sixteenth time.
    pub(crate) fn derived(&self) -> &Derived {
        self.frame_cache
            .as_ref()
            .expect("App::derived() outside draw — use derive()")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `Derived`: a frame that renders the header, the
    /// board and the footer derives its lists ONCE. A `derive()` per widget
    /// reads >1 here; a `draw` that forgot to fill the cache reads 0 (and
    /// every `derived()` under it panics).
    #[test]
    fn draw_derives_once_per_frame() {
        let mut app = crate::app::tests::app_with(crate::app::tests::six_live(), vec![]);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        DERIVE_COUNT.with(|c| c.set(0));
        term.draw(|f| app.draw(f)).unwrap();
        assert_eq!(
            DERIVE_COUNT.with(|c| c.get()),
            1,
            "one derivation per draw, not one per widget"
        );
    }

    /// The sections partition the board and concatenate into the selection —
    /// the "one list" property. An older test proved
    /// the same thing about two implementations of the same lists; there is
    /// only one implementation now, so what is left to prove is the shape:
    /// nothing is in two sections, nothing on the board is in none, and
    /// `selection` is exactly their concatenation.
    #[test]
    fn the_sections_partition_the_board_in_selection_order() {
        use crate::app::tests::{app_with, g};
        use crate::config::{Favorite, Pin};
        use crate::domain::{League, Status};

        let ids = |games: &[Game]| -> Vec<String> { games.iter().map(|g| g.id.clone()).collect() };
        let mut games = vec![
            g("live1", "KC", "TB", true),
            g("live2", "DAL", "PHI", true),
            g("pre1", "NE", "SEA", false),
            g("fin1", "GB", "CHI", false),
        ];
        games[3].status = Status::Final;

        let mut app = app_with(
            games.clone(),
            vec![Pin {
                game_id: "live2".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        app.config.favorites.push(Favorite {
            league: League::Nfl,
            team_abbr: "NE".into(),
        });
        let d = app.derive();

        assert_eq!(
            ids(&d.my_games),
            vec!["live2", "pre1"],
            "pins then favorites"
        );
        assert_eq!(ids(&d.in_play), vec!["live1"], "the band is not in play");
        assert_eq!(ids(&d.finals), vec!["fin1"]);
        assert!(d.later.is_empty(), "pre1 is a my-game: {:?}", ids(&d.later));
        assert_eq!(
            ids(&d.selection),
            vec!["live2", "pre1", "live1", "fin1"],
            "selection is my_games ++ in_play ++ finals ++ later"
        );
        // The partition invariant `selection_len()` takes the shortcut on:
        // every visible game is in exactly one section, so the selection is
        // as long as the visible board and holds no duplicates.
        let mut unique = ids(&d.selection);
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), d.selection.len(), "no game in two sections");
        assert_eq!(d.selection.len(), app.visible_games().len());
        assert_eq!(app.selection_len(), d.selection.len());
        // The hero: live2 is pinned AND live, so it leads.
        assert_eq!(d.hero_id.as_deref(), Some("live2"));
        assert!(!d.mixed, "one league on the board");

        // A band whose top game is not live hands the hero to the ranking.
        let mut app = app_with(
            games,
            vec![Pin {
                game_id: "pre1".into(),
                league: League::Nfl,
                final_at: None,
            }],
        );
        app.tab = Tab::League(League::Nfl);
        let d = app.derive();
        assert_eq!(ids(&d.my_games), vec!["pre1"]);
        assert_eq!(
            d.hero_id.as_deref(),
            Some("live1"),
            "a pre-game pin never takes the hero"
        );
    }

    /// `derived()` is a frame-scoped borrow, and saying so out loud beats a
    /// stale list: outside a draw it names the fix in the panic.
    #[test]
    #[should_panic(expected = "App::derived() outside draw")]
    fn derived_outside_draw_panics() {
        let app = crate::app::tests::app_with(crate::app::tests::six_live(), vec![]);
        let _ = app.derived();
    }
}
