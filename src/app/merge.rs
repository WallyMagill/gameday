//! The data merge path: what a fetched board, summary, box score or
//! standings table does to `App`, and the fetch targets and errors that go
//! with them.

use super::App;
use crate::config::prune_pins;
use crate::domain::{Game, GameStats, League, Play, StandingsTable, Status, Summary};
use crate::views::View;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// One pending catch-up (spec §3.2): a game whose score moved while the
/// scoreboard's last play was not the scoring play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CatchupEntry {
    pub league: League,
    pub game_id: String,
    pub seq: u64,
    pub queued_tick: u64,
    /// Summaries this entry has already asked for and been answered by
    /// without learning the run. Bounded by `CATCHUP_MAX_ATTEMPTS`.
    pub attempts: u8,
    /// The score this ask is chasing: `(away, home)` as the board reported it
    /// when the delta landed. The summary's answer is the oldest play whose
    /// `score_after` reaches it — see [`resolve_catchup`].
    pub target: (u16, u16),
    /// The earliest tick this entry may be published to the scheduler again.
    /// 0 on the first queue — the first ask goes out at once. A retry sets it
    /// a full live poll ahead, because the lag being waited out is a poll:
    /// ESPN serves the same summary body until its play-by-play advances, so
    /// three asks fired inside a second are three asks at one payload.
    pub next_ask_tick: u64,
}

/// What a summary said about the catch-up that asked for it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CatchupOutcome {
    /// This run is the one the delta reported: cut on it.
    Cut(Play),
    /// The run is accounted for — the scoreboard's own row captured it before
    /// the summary arrived. Retire the ask, say nothing.
    Answered,
    /// The play-by-play has not reached the delta yet. Ask again (bounded).
    AskAgain,
}

/// Resolve one queued catch-up against the summary's scoring plays, and trim
/// them to the history this ask is allowed to leave on the board. `plays` is
/// oldest-first; `known` is what the game already carried.
///
/// The run that answers a delta is the OLDEST unknown play whose
/// `score_after` reaches `target` on both components — at or beyond, never
/// exactly equal. Football is why: ESPN folds the extra point into the
/// touchdown row and reports the post-kick score, so a poll that catches the
/// board at 0-6 asks about a score no row will ever carry
/// (`fixtures/nfl_summary_full.json` has the London touchdown at 0-7 and no
/// 0-6 row anywhere). Equality there matched nothing, burned every attempt,
/// and dropped the ask — the cut arrived a poll late, on the next delta.
///
/// Oldest, not newest, so a summary that runs past the delta cannot announce
/// a run the board has not reported: everything after the answer is trimmed
/// off and stays unknown, which is what lets the NEXT delta claim it. Plays
/// before the answer are history and are kept, with no cut of their own.
///
/// The answer being already known means the scoreboard beat the summary to
/// it. That is `Answered`, not another cut and not a retry — and it is why
/// this looks for the oldest play reaching the target rather than the oldest
/// *unknown* one: skipping past a known answer would cut on the run after it,
/// which is exactly the future run this rule exists to refuse.
pub(crate) fn resolve_catchup(
    plays: &mut Vec<Play>,
    target: (u16, u16),
    known: &[Play],
) -> CatchupOutcome {
    let reaches = |p: &Play| {
        p.score_after
            .is_some_and(|s| s.0 >= target.0 && s.1 >= target.1)
    };
    let Some(at) = plays.iter().position(reaches) else {
        // Nothing in this feed reaches the score the board is already
        // showing: the play-by-play is behind. Everything here is history.
        return CatchupOutcome::AskAgain;
    };
    let answer = plays[at].clone();
    plays.truncate(at + 1);
    if known.iter().any(|k| same_play(k, &answer)) {
        CatchupOutcome::Answered
    } else {
        CatchupOutcome::Cut(answer)
    }
}

/// Play identity: ESPN ids when both sides carry one, else the text (demo
/// and sim plays have no ids). Never compare a real id against an empty one.
pub(crate) fn same_play(a: &Play, b: &Play) -> bool {
    if !a.id.is_empty() && !b.id.is_empty() {
        a.id == b.id
    } else {
        a.text == b.text
    }
}

impl App {
    pub fn apply_boards(&mut self, league: League, mut games: Vec<Game>, stale: bool) {
        let now = OffsetDateTime::now_utc();
        let prev_board = self.boards.get(&league).cloned().unwrap_or_default();
        // Score-change flash fires ONLY here — from data. A first sighting
        // (startup, new game) seeds last_scores without flashing.
        for g in &mut games {
            // Carry the accumulated scoring plays across the wholesale
            // replace — and the NHL strength only a summary
            // can produce. The scoreboard carries no strength field at all,
            // so without this carry the zoom's power-play chip and penalty
            // meter (both derived from `Extras::Hockey`) would blink out on
            // every scoreboard poll and back in on the next summary — a 15s
            // flicker for the length of the power play.
            //
            // NHL and live only, deliberately: MLB and soccer extras are
            // scoreboard-owned, and a game that just went Final has no power
            // play to still be on.
            if let Some(prev) = prev_board.iter().find(|p| p.id == g.id) {
                if g.scoring_plays.is_empty() {
                    g.scoring_plays = prev.scoring_plays.clone();
                }
                if g.league == League::Nhl
                    && g.status == Status::Live
                    && g.extras == crate::domain::Extras::None
                {
                    g.extras = prev.extras.clone();
                }
            }
            // A cached payload is an OLDER snapshot, not news: its diff
            // against the last fresh scores is backwards and its lastPlay is
            // whatever was on screen then. Capturing that would write a bogus
            // scoring play that outlives the outage, so a stale apply is
            // scores-only — no flash, no capture, no last_scores rewrite.
            if stale {
                continue;
            }
            let score = (g.away_score, g.home_score);
            if let Some(prev) = self.last_scores.get(&g.id) {
                if *prev != score {
                    self.flashes.insert(g.id.clone(), self.tick);
                    // The scoreboard's last play at the moment the score
                    // moved is the scoring play only when ESPN marks it so.
                    // At a 15s cadence it is routinely the next pitch or
                    // snap (the review saw `HOME RUN · MIA FOUL`), and a
                    // real feed marks almost nothing: so the unmarked case
                    // asks the summary, which is the authority, once.
                    match g.last_plays.first() {
                        Some(p) if p.scoring => {
                            if !g.scoring_plays.iter().any(|s| same_play(s, p)) {
                                let p = p.clone();
                                g.scoring_plays.push(p.clone());
                                self.fire_cut(&g.id, &p, self.cut_is_full(g));
                            }
                        }
                        _ => {
                            match self.catchup.iter_mut().find(|c| c.game_id == g.id) {
                                // Still one ask per game — but it chases the
                                // score on the board NOW. A game that scores
                                // twice while its summary is in flight would
                                // otherwise have the older score as its
                                // target, cut on the older run, and leave the
                                // newer one with no ask of its own.
                                Some(pending) => {
                                    pending.target = score;
                                    // A fresh score is a fresh question: the
                                    // attempts spent chasing the old one do
                                    // not count against it. `seq` and
                                    // `queued_tick` stand — the request in
                                    // flight still answers, and the TTL runs
                                    // from the first delta of the run.
                                    pending.attempts = 0;
                                }
                                None => {
                                    self.catchup_seq += 1;
                                    self.catchup.push(CatchupEntry {
                                        league: g.league,
                                        game_id: g.id.clone(),
                                        seq: self.catchup_seq,
                                        queued_tick: self.tick,
                                        attempts: 0,
                                        target: score,
                                        next_ask_tick: 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            self.last_scores.insert(g.id.clone(), score);
        }
        for pin in &mut self.pins {
            if pin.final_at.is_none()
                && games
                    .iter()
                    .any(|g| g.id == pin.game_id && g.status == Status::Final)
            {
                pin.final_at = Some(now);
            }
        }
        self.boards.insert(league, games);
        // Favorite-score alerts diff the freshly merged boards; a hit starts
        // the header banner and queues the bell for main to ring. A cached
        // payload is skipped whole — not checked and discarded: AlertState
        // diffs on inequality, so an older snapshot reads as a score change
        // (a banner and a bell for a score going backwards), and consuming
        // its delta would swallow the real one when the fresh board lands.
        if !stale {
            if let Some(alert) = self
                .alerts
                .check(&self.config.favorites, &self.boards, self.tick)
            {
                self.active_alert = Some(alert);
                self.bell_pending = true;
            }
            // Fresh data landed: the live band may re-sort, if the data that
            // decides the order actually moved (`maybe_reorder`). This sits
            // inside the `!stale` guard on purpose — a cached payload is not
            // news and must never move the board.
            self.maybe_reorder();
            // …and TV lets go of anything that just left the live slate.
            // After `maybe_reorder`, so the hero it re-anchors to is
            // this event's, not the last one's.
            self.tv_hygiene();
        }
        // Drop score memory for games no board carries any more: unbounded
        // growth over a days-long session, and a recycled id would flash on
        // first sighting instead of seeding silently.
        let mut last_scores = std::mem::take(&mut self.last_scores);
        last_scores.retain(|id, _| self.boards.values().flatten().any(|g| g.id == *id));
        self.last_scores = last_scores;
        let mut stats = std::mem::take(&mut self.stats);
        stats.retain(|id, _| self.boards.values().flatten().any(|g| g.id == *id));
        self.stats = stats;
        self.net.ok(Instant::now(), stale);
        self.pins = prune_pins(std::mem::take(&mut self.pins), now);
        self.persist_pins_quiet();
        self.clamp_selected();
    }

    /// Fire a cut unless the view suppresses them; a takeover rings the bell.
    /// Size is decided by the caller's `full`, not in `CutState` —
    /// pinned/favorited/TV takes the screen, everything else is the quiet
    /// band.
    fn fire_cut(&mut self, game_id: &str, play: &Play, full: bool) {
        if self.cut_suppressed() {
            return;
        }
        self.cuts_fired_count += 1;
        self.cuts.fire(game_id, play, full, self.tick);
        if full {
            // Only a takeover rings; the band is quiet by definition.
            self.bell_pending = true;
        }
    }

    /// A fetched non-today slate. Replaces wholesale (a dated board is a
    /// snapshot) and never touches flash/score state — traveled slates are
    /// read-only history/preview, not live data.
    pub fn merge_dated_board(&mut self, league: League, date: time::Date, games: Vec<Game>) {
        self.clear_aux_error(league, "dated");
        self.dated_boards.insert((league, date), games);
        self.clamp_selected();
    }

    pub fn merge_summary(&mut self, game_id: &str, summary: Summary) {
        if let Some(league) = self.league_of(game_id) {
            self.clear_aux_error(league, "summary");
        }
        // A summary lands only for the zoomed game or a queued catch-up, and
        // it can carry a scoring play the scoreboard never showed us. That is
        // news exactly once: the play that is new to `game.scoring_plays`
        // fires a cut, and the rest of the list is history being backfilled.
        //
        // The catch-up entry is consumed by the ANSWER, not by the ask: a
        // summary that did not name the run is asked again (`retry_catchup`),
        // because ESPN publishes the score before the play-by-play and the
        // fetch a score delta triggers can land in that gap. Two exits
        // consume it without an answer, both deliberate: a summary that
        // carried nothing at all past its attempt bound, and a game that has
        // left every board (nothing to cut on, and no board to cut over). The
        // index is taken before the boards loop borrows `self` mutably;
        // nothing between here and the exits touches `self.catchup`, so it
        // stays valid.
        let queued = self.catchup.iter().position(|c| c.game_id == game_id);
        // Non-football summaries carry no "drives", so they can map to zero
        // plays; keep the scoreboard's lastPlay instead of blanking the tile.
        if summary.last_plays.is_empty() && summary.scoring_plays.is_empty() {
            // A game that has left every board has nothing to cut on and no
            // board to cut over: two more asks would buy a score nobody can
            // see. Drop it here rather than spending the attempts.
            if let Some(i) = queued.filter(|_| self.league_of(game_id).is_none()) {
                self.catchup.remove(i);
            } else {
                self.retry_catchup(queued);
            }
            return;
        }
        // The score this catch-up is chasing, if one asked for this summary.
        let target = queued.map(|i| self.catchup[i].target);
        // The cut this summary earns, and whether the ask it answers is
        // finished with. Both decided inside the boards loop, acted on after
        // it (the loop holds `self.boards` mutably).
        let mut cut: Option<Play> = None;
        let mut ask_again = false;
        for board in self.boards.values_mut() {
            if let Some(game) = board.iter_mut().find(|g| g.id == game_id) {
                let known: Vec<Play> = game.scoring_plays.clone();
                if !summary.scoring_plays.is_empty() {
                    // Summary order differs by source: football's
                    // `scoringPlays` is oldest-first, a list derived from the
                    // play-by-play is newest-first. Normalize to oldest-first
                    // by asking `last_plays` (newest-first) where the ends of
                    // the list sit — a smaller index means newer.
                    let mut sp = summary.scoring_plays.clone();
                    let newest_first = sp.len() > 1 && {
                        let pos =
                            |q: &Play| summary.last_plays.iter().position(|p| same_play(p, q));
                        match (pos(&sp[0]), pos(&sp[sp.len() - 1])) {
                            (Some(a), Some(b)) => a < b,
                            // Nothing to compare against: ESPN's own
                            // `scoringPlays` is oldest-first already.
                            _ => false,
                        }
                    };
                    if newest_first {
                        sp.reverse();
                    }
                    game.scoring_plays = sp;
                }
                // Per-sport facts only the summary carries (NHL strength +
                // penalties). `Extras::None` is "this summary had nothing to
                // say", never an instruction to erase what the scoreboard
                // mapped. The meter that rides these is NOT stored on the
                // game — the zoom derives it.
                if summary.extras != crate::domain::Extras::None {
                    game.extras = summary.extras.clone();
                }
                if !summary.last_plays.is_empty() {
                    let mut last_plays = summary.last_plays;
                    for play in &mut last_plays {
                        if summary.scoring_plays.iter().any(|s| same_play(s, play)) {
                            play.scoring = true;
                        }
                    }
                    game.last_plays = last_plays;
                }
                // Which play the cut names. A queued ask that has a running
                // score to compare against resolves against it; everything
                // else (soccer, and the zoom's own cadence) keeps the older
                // newest-unseen rule.
                let scored_feed = game.scoring_plays.iter().any(|p| p.score_after.is_some());
                match target.filter(|_| scored_feed) {
                    Some(t) => match resolve_catchup(&mut game.scoring_plays, t, &known) {
                        CatchupOutcome::Cut(play) => cut = Some(play),
                        CatchupOutcome::Answered => {}
                        CatchupOutcome::AskAgain => ask_again = true,
                    },
                    None => {
                        if queued.is_some() || !known.is_empty() {
                            // Soccer's keyEvents carry no running score, and a
                            // zoom's summary answers no ask: newest unseen. A
                            // first summary with nothing queued is history
                            // being backfilled, never a cut.
                            cut = game
                                .scoring_plays
                                .iter()
                                .rev()
                                .find(|p| !known.iter().any(|k| same_play(k, p)))
                                .cloned();
                            ask_again = queued.is_some() && cut.is_none();
                        }
                    }
                }
                break;
            }
        }
        if ask_again {
            self.retry_catchup(queued);
        } else if let Some(i) = queued {
            // Answered — with a cut or with "you already have it".
            self.catchup.remove(i);
        }
        if let Some(play) = cut {
            let full = self
                .game_by_id(game_id)
                .is_some_and(|g| self.cut_is_full(&g));
            self.fire_cut(game_id, &play, full);
        }
        // No reorder here, and nothing a summary carries can cause one
        // elsewhere either. The rank fingerprint is (scores, status, hot),
        // all three scoreboard-owned; the one thing a summary now adds that
        // rank could have read — the NHL strength — reaches no meter
        // field and is refused by `watchability`'s NHL arm besides,
        // precisely so zooming a game can never move it. Zoom and unzoom
        // leave the order exactly where the last scoreboard apply put it.
    }

    /// A catch-up whose summary did not name the run: ask again, up to
    /// [`CATCHUP_MAX_ATTEMPTS`] times, then let it go.
    ///
    /// ESPN moves the score before it moves the play-by-play. The 2026-09-07
    /// replay capture is the receipt: `mlb-20260907-0334` poll 19 already
    /// reports 4-2 while `situation.lastPlay` is still the pitch before
    /// "Edman doubled to left, Muncy scored." A summary fetched in that gap
    /// carries nothing the app has not already seen, and consuming the entry
    /// there — what this did before — meant that run never got a cut at all.
    ///
    /// Bumping `seq` is what re-arms the ask: the scheduler emits one
    /// `Summary` per sequence number it has not seen (`poll::Scheduler::due`),
    /// so a new number is a new request and the old one is not retried
    /// forever by accident. Bounded three ways — the attempt count, the TTL,
    /// and the one-entry-per-game rule that was already here.
    ///
    /// …and paced by a fourth: the re-armed entry is withheld from
    /// `catchup_wants` for one live poll. The gap being waited out is a poll
    /// (`mlb-20260907-0334` poll 19 lacks the play poll 20 has), and the UI
    /// publishes its wants every 200 ms, so an unpaced ladder spends all
    /// three attempts inside two seconds — three questions to a payload ESPN
    /// has not changed (a 304 hands back the same cached body).
    ///
    /// `None` (no entry for this game) is the zoom's own summary cadence
    /// landing: nothing was asked, so nothing is retried.
    fn retry_catchup(&mut self, queued: Option<usize>) {
        let Some(i) = queued else { return };
        let attempts = self.catchup[i].attempts + 1;
        if attempts >= crate::app::CATCHUP_MAX_ATTEMPTS {
            // A limit someone can hit is a limit they have to be able to see:
            // this is the one path where a real score gets no cut at all, and
            // without a line it is indistinguishable from a score the app
            // never noticed.
            let dropped = self.catchup.remove(i);
            crate::log::note(&format!(
                "catch-up dropped: game={} target={}-{} attempts={attempts} (max {})",
                dropped.game_id,
                dropped.target.0,
                dropped.target.1,
                crate::app::CATCHUP_MAX_ATTEMPTS,
            ));
            return;
        }
        self.catchup_seq += 1;
        self.catchup[i].attempts = attempts;
        self.catchup[i].seq = self.catchup_seq;
        // One live scoreboard poll out: the same 15 s the board itself waits
        // for new data. Withholding the entry leaves `last_catchup_seq`
        // un-advanced in the scheduler, so the request goes out the moment
        // the entry reappears — no scheduler change, and no timer of its own.
        self.catchup[i].next_ask_tick =
            self.tick + crate::poll::SCOREBOARD_LIVE.as_secs() * crate::app::LIVE_TICKS_PER_SEC;
    }

    /// The zoomed game's (league, id) — the stats poll's only target. None
    /// unless the Zoom view is open and its game is still on a board.
    pub fn stats_target(&self) -> Option<(League, String)> {
        self.zoomed_game().map(|g| (g.league, g.id))
    }

    /// Latest box score for `game_id`, from the stats poll (or a fixture in
    /// tests/dump). Replaces wholesale — rows are a snapshot, not a delta.
    pub fn merge_stats(&mut self, game_id: &str, stats: GameStats) {
        if let Some(league) = self.league_of(game_id) {
            self.clear_aux_error(league, "stats");
        }
        self.stats.insert(game_id.to_string(), stats);
    }

    /// Record a failed scoreboard fetch: which league, the provider's short
    /// error (`ESPN 403 nfl scoreboard`), and how long until the scheduler
    /// retries. The chip and the board message read it through `net`.
    pub fn note_failure(&mut self, league: League, error: String, retry_in: Option<Duration>) {
        self.net.failed(Instant::now(), error.clone(), retry_in);
        // A populated board keeps its scores and the header chip says the
        // rest — a toast on top would nag. With nothing on the board, the
        // failure IS the news, so it also gets the footer line.
        if !self.boards.values().any(|b| !b.is_empty()) {
            self.status_line = Some(match retry_in {
                Some(d) => format!("{} · {} · retry in {}s", league.slug(), error, d.as_secs()),
                None => format!("{} · {error}", league.slug()),
            });
        }
    }

    /// The league the Standings view wants a table for — the on-demand
    /// standings fetch's only target. None unless the view is open.
    pub fn standings_target(&self) -> Option<League> {
        match self.view {
            View::Standings(league) => Some(league),
            _ => None,
        }
    }

    /// Latest standings for one league, from the on-demand fetch (or a
    /// fixture in tests/dump). Replaces wholesale — a table is a snapshot.
    /// Stamped with the moment we took it: a table the feed doesn't label
    /// with a season is labeled with its own age instead, so it never reads
    /// as live when it isn't.
    /// How old the last fresh board may get before the header stops claiming
    /// the numbers are live — derived from the cadence actually in use, so
    /// the chip can never contradict the scheduler. Live: 3 × the 15 s live
    /// cadence (one missed poll is noise, three in a row is a problem). Idle:
    /// one 60 s cadence plus one live window, because at a minute between
    /// polls a 45 s cutoff would call every healthy board stale.
    pub fn stale_after(&self) -> Duration {
        if self.any_live() {
            3 * crate::poll::SCOREBOARD_LIVE
        } else {
            crate::poll::SCOREBOARD_IDLE + crate::poll::SCOREBOARD_LIVE
        }
    }

    pub fn merge_standings(&mut self, mut table: StandingsTable) {
        table.fetched_at = Some(self.now());
        self.clear_aux_error(table.league, "standings");
        self.standings.insert(table.league, table);
    }

    /// An on-demand fetch failed. `what` names the request kind, so the view
    /// that asked can say which fetch is missing rather than showing an empty
    /// pane that reads like "no data exists".
    pub fn note_aux_failure(&mut self, league: League, what: &'static str, error: String) {
        self.aux_errors.insert((league, what), error);
    }

    /// The matching success: the error stops being true the moment data lands.
    pub fn clear_aux_error(&mut self, league: League, what: &'static str) {
        self.aux_errors.remove(&(league, what));
    }

    pub fn aux_error(&self, league: League, what: &'static str) -> Option<&str> {
        self.aux_errors.get(&(league, what)).map(|s| s.as_str())
    }

    /// Which board carries `game_id` — the zoom-driven fetches (summary,
    /// stats) are addressed by game id, and `aux_errors` is keyed by league.
    fn league_of(&self, game_id: &str) -> Option<League> {
        self.boards
            .iter()
            .find(|(_, games)| games.iter().any(|g| g.id == game_id))
            .map(|(league, _)| *league)
    }
}
