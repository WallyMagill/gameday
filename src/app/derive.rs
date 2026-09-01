//! The derived game lists — every list a frame renders, and the `Derived`
//! bundle that evaluates them once per draw instead of once per widget.
//!
//! They stay methods on `App` because the key handlers call them outside a
//! draw (clamping the selection, paging, scrolling the feed); `derive()`
//! runs all of them together and `App::draw` parks the result in
//! `frame_cache` for the widgets to read through `derived()`.

use super::{App, Tab};
use crate::domain::{Game, League, Status};
use crate::home::home_games;

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

    /// Scoring plays of the ticker's live games, board order.
    pub(crate) fn ticker_events(&self) -> Vec<(Game, crate::domain::Play)> {
        let live = self.ticker_live();
        self.scoring_events()
            .into_iter()
            .filter(|(g, _)| live.iter().any(|l| l.id == g.id))
            .collect()
    }
}
