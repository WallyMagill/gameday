//! The live band's ordering: the fingerprint that decides when a re-sort is
//! news, and the two entry points that hand a fresh live slate to
//! `OrderState`.

use super::App;
use crate::domain::{Game, Status};
use std::collections::HashMap;

/// One live game's ordering identity (R24): away score, home score, status,
/// the hot flag, and soccer's on-field count. Two equal fingerprints mean
/// nothing the board sorts on has moved, so the order is left alone.
pub(super) type RankFingerprint = (u16, u16, Status, bool, Option<(u8, u8)>);

impl App {
    /// Every live game on an enabled board, minus the viewer's own. Pins AND
    /// favorites live in the MY GAMES band and never re-sort (spec §1, ruling
    /// R26), so `OrderState` is never told about either.
    pub fn live_all(&self) -> Vec<Game> {
        self.config
            .enabled_tabs
            .iter()
            .filter_map(|l| self.boards.get(l))
            .flatten()
            .filter(|g| g.status == Status::Live)
            .filter(|g| !self.is_my_game(g))
            .cloned()
            .collect()
    }

    /// Re-sort the live band, but only if the data behind the order actually
    /// moved: the id set changed, or some game's score, status or hot flag
    /// did. A clock that merely advanced is not an event (R24) — spec §2:
    /// "Between events the order is frozen even though L keeps rising."
    pub(super) fn maybe_reorder(&mut self) {
        let live = self.live_all();
        let now = self.now();
        let fps: HashMap<String, RankFingerprint> = live
            .iter()
            .map(|g| {
                (
                    g.id.clone(),
                    (
                        g.away_score,
                        g.home_score,
                        g.status,
                        crate::rank::watchability(g, now).hot,
                        match &g.extras {
                            crate::domain::Extras::Soccer { men, .. } => *men,
                            _ => None,
                        },
                    ),
                )
            })
            .collect();
        let changed = fps.len() != self.rank_fingerprints.len()
            || fps
                .iter()
                .any(|(id, f)| self.rank_fingerprints.get(id) != Some(f));
        if changed {
            self.order.on_event(
                &live,
                self.config.sort,
                &self.config.enabled_tabs,
                now,
                self.tick,
            );
            self.tv_follow();
        }
        self.rank_fingerprints = fps;
    }

    /// A sort-key change (`s`, `:sort`, the config editor's SORT row) is a
    /// real event on its own: unlike a clock tick, it must re-derive the
    /// order right away rather than waiting for `maybe_reorder`'s fingerprint
    /// gate to see a score/status change.
    pub fn force_reorder(&mut self) {
        let live = self.live_all();
        let now = self.now();
        self.order.on_event(
            &live,
            self.config.sort,
            &self.config.enabled_tabs,
            now,
            self.tick,
        );
        // A new sort key is a new ranking, and TV shows the ranking's top.
        self.tv_follow();
    }
}
