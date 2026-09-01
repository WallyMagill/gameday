//! The derived game lists — every list a frame renders, and the `Derived`
//! bundle that evaluates them once per draw instead of once per widget.
//!
//! They stay methods on `App` because the key handlers call them outside a
//! draw (clamping the selection, paging, scrolling the feed); `derive()`
//! runs all of them together and `App::draw` parks the result in
//! `frame_cache` for the widgets to read through `derived()`.

use super::{App, Tab};
use crate::domain::{Game, League, Play, Status};
use crate::home::home_games;
use std::cell::Cell;

thread_local! {
    /// How many times [`App::derive`] has run on this thread. Only the
    /// once-per-frame test reads it; the counter itself is a `Cell` bump,
    /// too cheap to be worth compiling out.
    pub(crate) static DERIVE_COUNT: Cell<u32> = const { Cell::new(0) };
}

/// Every game list one frame renders, evaluated together. Before this, each
/// widget called the list fns itself and a single draw re-cloned the boards
/// about fifteen times; now `App::draw` derives once and parks the result in
/// `App::frame_cache` for the widgets to borrow.
pub struct Derived {
    pub visible: Vec<Game>,
    pub live: Vec<Game>,
    pub slate: Vec<Game>,
    pub mosaic: Vec<Game>,
    pub selection: Vec<Game>,
    pub scoring: Vec<(Game, Play)>,
    pub ticker_live: Vec<Game>,
    pub ticker_events: Vec<(Game, Play)>,
}

/// Does either team match the `/` filter? Case-insensitive substring on
/// abbr ("KC"), location ("KANSAS CITY"), and name ("Chiefs").
fn game_matches(game: &Game, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    [&game.away, &game.home].into_iter().any(|t| {
        t.abbr.to_lowercase().contains(&needle)
            || t.location.to_lowercase().contains(&needle)
            || t.name.to_lowercase().contains(&needle)
    })
}

impl App {
    /// The soonest scheduled start still ahead of us across the enabled
    /// boards — what empty Home names when nothing is live.
    pub fn next_start(&self) -> Option<Game> {
        let now = self.now();
        self.concat_boards()
            .into_iter()
            .filter(|g| g.status == Status::Pre && g.start.is_some_and(|s| s > now))
            .min_by_key(|g| g.start.expect("filtered to Some above"))
    }

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
        match self.active_filter() {
            Some(needle) => games
                .into_iter()
                .filter(|g| game_matches(g, needle))
                .collect(),
            None => games,
        }
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

    pub fn live_games(&self) -> Vec<Game> {
        self.visible_games()
            .into_iter()
            .filter(|g| g.status == Status::Live)
            .collect()
    }

    pub fn slate_games(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => vec![],
            Tab::League(_) => self
                .visible_games()
                .into_iter()
                .filter(|g| g.status == Status::Pre || g.status == Status::Final)
                .collect(),
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

    /// Everything j/k can land on. On a league tab the selection runs through
    /// the live mosaic tiles first, then continues into the slate rows below.
    pub(crate) fn selection_list(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let mut list = self.live_games();
                list.extend(self.slate_games());
                list
            }
        }
    }

    pub(crate) fn selected_game(&self) -> Option<Game> {
        let list = self.selection_list();
        list.get(self.selected).cloned()
    }

    /// Scoring plays across every enabled board, newest first per game,
    /// games in board order. Finals keep theirs until they leave the board.
    pub(crate) fn scoring_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let mut out = Vec::new();
        for game in self.concat_boards() {
            for play in game.scoring_plays.iter().rev() {
                out.push((game.clone(), play.clone()));
            }
        }
        out
    }

    /// Games shown as mosaic tiles. With no live games on a league tab the
    /// slate games fill the mosaic as tiles — never a blank pane — while the
    /// slate strip below still lists them departure-board style.
    pub(crate) fn mosaic_games(&self) -> Vec<Game> {
        match self.tab {
            Tab::Home => self.visible_games(),
            Tab::League(_) => {
                let live = self.live_games();
                if !live.is_empty() {
                    live
                } else {
                    self.slate_games()
                }
            }
        }
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

    /// Every live game across the enabled boards, league order — the ticker
    /// covers what the visible tab (or a traveled date) does not. A typed
    /// filter is explicit intent, so it narrows the ticker too.
    pub(crate) fn ticker_live(&self) -> Vec<Game> {
        let needle = self.active_filter();
        self.concat_boards()
            .into_iter()
            .filter(|g| g.status == Status::Live)
            .filter(|g| needle.is_none_or(|n| game_matches(g, n)))
            .collect()
    }

    /// Every list for one frame, built from each other rather than from the
    /// boards: `visible` is the only walk of the enabled boards the tab
    /// lists need, and `live`/`slate`/`mosaic`/`selection` are all filters of
    /// it. Definitionally the same lists the fns above return — they are the
    /// spec, this is the shared evaluation.
    pub(crate) fn derive(&self) -> Derived {
        DERIVE_COUNT.with(|c| c.set(c.get() + 1));
        let visible = self.visible_games();
        let live: Vec<Game> = visible
            .iter()
            .filter(|g| g.status == Status::Live)
            .cloned()
            .collect();
        let slate: Vec<Game> = match self.tab {
            Tab::Home => Vec::new(),
            Tab::League(_) => visible
                .iter()
                .filter(|g| g.status == Status::Pre || g.status == Status::Final)
                .cloned()
                .collect(),
        };
        let mosaic = match self.tab {
            Tab::Home => visible.clone(),
            Tab::League(_) if live.is_empty() => slate.clone(),
            Tab::League(_) => live.clone(),
        };
        let selection = match self.tab {
            Tab::Home => visible.clone(),
            Tab::League(_) => {
                let mut list = live.clone();
                list.extend(slate.iter().cloned());
                list
            }
        };
        let scoring = self.scoring_events();
        let ticker_live = self.ticker_live();
        // Scoring plays of the ticker's live games, board order.
        let ticker_events = scoring
            .iter()
            .filter(|(g, _)| ticker_live.iter().any(|l| l.id == g.id))
            .cloned()
            .collect();
        Derived {
            visible,
            live,
            slate,
            mosaic,
            selection,
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
            .expect("App::derived() outside draw — use the list fns")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `Derived`: a frame that renders the header, the
    /// mosaic, the slate, the sidebar, the ticker and the footer derives its
    /// lists ONCE. A `derive()` per widget reads >1 here; a `draw` that
    /// forgot to fill the cache reads 0 (and every `derived()` under it
    /// panics). The widgets can't re-derive behind its back either — the
    /// only other way to a list is the fns, and inside a draw they have no
    /// callers left.
    #[test]
    fn draw_derives_once_per_frame() {
        let mut app = crate::app::tests::app_with(crate::app::tests::six_live(), vec![]);
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        DERIVE_COUNT.with(|c| c.set(0));
        term.draw(|f| app.draw(f)).unwrap();
        assert_eq!(
            DERIVE_COUNT.with(|c| c.get()),
            1,
            "one derivation per draw, not one per widget"
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
