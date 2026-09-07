//! Replay of consecutive real scoreboard polls through the real apply path.
//! Every sequence under fixtures/replay/ is one live window captured by
//! scripts/capture-replay.sh. The assertion is the one the 2026-09-05 review
//! found broken on live data: when a score moves, the cut names the play
//! that scored — never the pitch or snap the poll happened to catch.
//!
//! One rule this harness does NOT prove: that a catch-up matches at or beyond
//! its target rather than exactly. Baseball has no extra point, so no MLB
//! window can exercise it; the football unit test
//! (`a_touchdown_row_carries_its_extra_point_so_the_catchup_matches_at_or_beyond`)
//! is where that lives. Do not "strengthen" the harness for it — a captured
//! window either contains the case or it does not.
use gameday::app::{App, LIVE_TICKS_PER_SEC};
use gameday::config::Config;
use gameday::domain::*;
use gameday::provider::map::{map_scoreboard, map_summary};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Render ticks between two polls: the live cadence is 15 s and the loop runs
/// at [`LIVE_TICKS_PER_SEC`] while anything is live. Spending exactly that
/// many per poll is what makes the harness's clock the app's — the 600-tick
/// catch-up TTL is then four polls here, as it is in production, and a cut's
/// 15-tick band expires between polls instead of lingering into the next one.
const TICKS_PER_POLL: u64 = 15 * LIVE_TICKS_PER_SEC;

fn app(name: &str) -> App {
    let dir = std::env::temp_dir().join(format!("gd-replay-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
    app.now_override = Some(time::OffsetDateTime::now_utc());
    // Past the startup grace. `cut_suppressed` refuses every cut for the
    // first 30 s (300 ticks at LIVE_TICKS_PER_SEC = 10) so a session that
    // opens onto a scoring play does not shout history at you. A replay
    // starts mid-window by construction, so it starts past it — at tick 0
    // this harness would assert that nothing ever happens.
    app.tick = 400;
    app
}

fn sequences() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/replay");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("fixtures/replay must exist: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(
        !dirs.is_empty(),
        "no replay sequences under {}: the harness must not pass vacuously",
        root.display()
    );
    dirs
}

fn league_of(dir: &Path) -> League {
    let name = dir.file_name().unwrap().to_string_lossy();
    let slug = name.split('-').next().unwrap();
    League::from_slug(slug)
        .unwrap_or_else(|| panic!("directory {name} does not start with a league slug"))
}

/// Every poll of a sequence, in capture order, as (file name, body). The name
/// rides along because a window can have gaps: `capture-replay.sh` keeps
/// polling when one fetch fails, so `007.json` may simply not exist and an
/// index would then name the wrong poll in a failure message.
fn polls(dir: &Path) -> Vec<(String, String)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .chars()
                .next()
                .unwrap()
                .is_ascii_digit()
        })
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(p).unwrap(),
            )
        })
        .collect()
}

fn summary_for(dir: &Path, id: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(format!("summary-{id}.json"))).ok()
}

/// The run that made the score `(away, home)`, and the captured feed
/// truncated to it.
struct Run {
    id: String,
    text: String,
    /// The summary as it stood the instant that run landed. The harness
    /// serves the captured file WHOLE — that is the point of matching the
    /// delta by score — and uses this only to build a feed that has *not*
    /// caught up yet, for the retry drill.
    feed_through: String,
}

/// The scoring row that leaves the score at `(away, home)` — the run a delta
/// to that score is reporting — and the feed truncated to it.
///
/// This is the harness's independent answer: it comes from the raw capture,
/// never from what the app chose. `awayScore`/`homeScore` on a summary play
/// is the score AFTER it, so the row whose pair equals the poll's score is
/// the run that made it, whatever else the file contains.
///
/// HISTORY. Each sequence's summary is fetched once, after the last poll, so
/// it carries every scoring play of the window — including runs that had not
/// happened when an earlier delta fired. Under the old newest-unseen rule
/// that broke both ways: poll 19 of mlb-20260907-0334 (4-2) cut on
/// "T. Hernández reached on infield single…", the 4-4 still in the future,
/// and poll 40 then fired nothing at all because its scoreboard row had
/// already been backfilled. The harness worked around it by serving a
/// truncated feed. `App` now matches the delta by score (`Play::score_after`),
/// so the workaround is gone: the main test below serves each captured
/// summary WHOLE, exactly as it sits on disk, and the truncation survives
/// only to build a feed that has not caught up yet for the retry drill.
///
/// Flat arrays only (`plays` for MLB/NBA/NHL, `scoringPlays` for football).
/// A football drill would also want `drives.previous[].plays` truncated —
/// nothing reads those for the cut, so that is left until a football window
/// is captured.
fn run_that_made(body: &str, (away, home): (u16, u16), ctx: &str) -> Run {
    let mut v: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{ctx}: {e}"));
    let mut named: Option<(String, String)> = None;
    for key in ["plays", "scoringPlays"] {
        let Some(rows) = v[key].as_array().cloned() else {
            continue;
        };
        let Some(at) = rows.iter().rposition(|p| {
            p["scoringPlay"].as_bool().unwrap_or(false)
                && p["awayScore"].as_u64() == Some(away as u64)
                && p["homeScore"].as_u64() == Some(home as u64)
        }) else {
            continue;
        };
        named = Some((
            rows[at]["id"].as_str().unwrap_or_default().to_string(),
            rows[at]["text"].as_str().unwrap_or_default().to_string(),
        ));
        v[key] = Value::Array(rows[..=at].to_vec());
    }
    let (id, text) = named.unwrap_or_else(|| {
        panic!(
            "{ctx}: no scoring row in the captured summary leaves the score {away}-{home}, \
             so nothing in the feed explains this delta (the summary is for another game, \
             or the window needs recapturing)"
        )
    });
    Run {
        id,
        text,
        feed_through: v.to_string(),
    }
}

/// The sequence's captured summary for `game_id`, read from disk once (they
/// run to megabytes) — the file as it is, which is what the app is served.
fn summary(dir: &Path, cache: &mut HashMap<String, String>, game_id: &str, ctx: &str) -> String {
    cache
        .entry(game_id.to_string())
        .or_insert_with(|| {
            summary_for(dir, game_id).unwrap_or_else(|| {
                panic!("{ctx}: the score moved but no summary-{game_id}.json was captured")
            })
        })
        .clone()
}

/// `run_that_made` against that captured summary.
fn run_for(
    dir: &Path,
    cache: &mut HashMap<String, String>,
    game_id: &str,
    score: (u16, u16),
    ctx: &str,
) -> Run {
    run_that_made(&summary(dir, cache, game_id, ctx), score, ctx)
}

/// The first poll of a sequence whose delta takes the catch-up path: the
/// score moved while the scoreboard's own last play was not marked scoring,
/// so `App` has to ask the summary. That is the poll the lag drill below
/// runs on.
struct Lag {
    at: usize,
    game: String,
    /// The score the poll before — the innings the app has already seen.
    before: (u16, u16),
    /// The score at the delta — the run the cut owes the viewer.
    after: (u16, u16),
}

fn first_catchup_delta(league: League, polls: &[(String, String)]) -> Option<Lag> {
    let mut prev: HashMap<String, (u16, u16)> = HashMap::new();
    for (at, (_, body)) in polls.iter().enumerate() {
        let games = map_scoreboard(league, body, time::UtcOffset::UTC)
            .unwrap_or_else(|e| panic!("poll {at} of this sequence does not map: {e}"));
        for g in &games {
            let score = (g.away_score, g.home_score);
            if let Some(before) = prev.get(&g.id).copied() {
                if before != score && !g.last_plays.first().is_some_and(|p| p.scoring) {
                    return Some(Lag {
                        at,
                        game: g.id.clone(),
                        before,
                        after: score,
                    });
                }
            }
            prev.insert(g.id.clone(), score);
        }
    }
    None
}

/// The case `CATCHUP_MAX_ATTEMPTS` exists for, replayed on real polls: ESPN
/// publishes the score before the play-by-play, so the summary a delta
/// triggers can arrive without the run in it. Served that lagging summary on
/// purpose, the app must fire nothing, keep the ask alive with a new sequence
/// number, and land the right cut on the next summary — one poll late.
///
/// The drill runs from a cold session on purpose — nothing captured, nothing
/// "already seen". That is the hard case: the lagging feed hands the app four
/// real scoring plays it has never seen, and the rule that survives it is the
/// one that asks which play produced THIS score. Newest-unseen would cut on
/// "Young singled to center, Ford scored." — the top of the 4th, the wrong
/// team's run, an inning and a half before the delta.
#[test]
fn a_summary_that_has_not_caught_up_is_asked_again_and_the_cut_still_names_the_run() {
    let mut drilled = 0;
    for dir in sequences() {
        let league = league_of(&dir);
        let label = dir.file_name().unwrap().to_string_lossy().into_owned();
        let all = polls(&dir);
        let Some(lag) = first_catchup_delta(league, &all) else {
            continue;
        };
        assert!(
            all.len() > lag.at + 1,
            "{label}: the catch-up delta is the last poll of the window, so there is no poll left for the retry to land in"
        );
        let mut app = app(&format!("{label}-lag"));
        let mut summaries: HashMap<String, String> = Default::default();
        // The sequence number of the first ask, read at the lag poll and
        // checked at the next one: the retry is paced a poll out, so the two
        // halves of that assertion sit in different iterations.
        let mut asked: Option<u64> = None;
        for (i, (name, body)) in all.iter().enumerate().take(lag.at + 2) {
            let games = map_scoreboard(league, body, time::UtcOffset::UTC)
                .unwrap_or_else(|e| panic!("{}: poll {name}: {e}", dir.display()));
            let ctx = format!("{}: poll {name}: {}", dir.display(), lag.game);
            let score_now = games
                .iter()
                .find(|g| g.id == lag.game)
                .map(|g| (g.away_score, g.home_score));
            for _ in 0..TICKS_PER_POLL {
                app.advance_tick();
            }
            let before = app.cuts_fired();
            app.apply_boards(league, games, false);
            let wants = app.catchup_wants();
            if i == lag.at {
                assert_eq!(wants.len(), 1, "{ctx}: the delta must queue a catch-up");
                // ESPN has the score but not the play yet: the feed at this
                // instant still stops at the previous score.
                let lagging = run_for(&dir, &mut summaries, &lag.game, lag.before, &ctx);
                let s = map_summary(league, &lagging.feed_through)
                    .unwrap_or_else(|e| panic!("{ctx}: {e}"));
                app.merge_summary(&lag.game, s);
                assert_eq!(
                    app.cuts_fired(),
                    before,
                    "{ctx}: the newest run this feed carries is {:?}, which is not the run that made it {}-{}",
                    lagging.text,
                    lag.after.0,
                    lag.after.1
                );
                // The re-armed ask is paced a poll out, so it is deliberately
                // NOT published in this same instant — the next poll's branch
                // is where it has to reappear, with a new sequence number.
                asked = Some(wants[0].seq);
                assert!(
                    app.catchup_wants().is_empty(),
                    "{ctx}: a retry fired in the same instant asks ESPN the same question twice for one payload"
                );
            } else if i == lag.at + 1 {
                assert_eq!(
                    score_now,
                    Some(lag.after),
                    "{ctx}: the score moved again inside the drill window; this sequence needs a different lag poll"
                );
                assert_eq!(
                    wants.len(),
                    1,
                    "{ctx}: the ask was consumed by a summary that did not answer it, so the run at {}-{} never gets a cut",
                    lag.after.0,
                    lag.after.1
                );
                let asked = asked.expect("the lag poll runs before this one");
                assert!(
                    wants[0].seq > asked,
                    "{ctx}: a retry needs a sequence number the scheduler has not seen (was {asked}, is {})",
                    wants[0].seq
                );
                // The retry, answered with the captured file whole — no
                // reconstruction: the score match is what keeps the runs it
                // carries from later innings out of this cut.
                let landed = run_for(&dir, &mut summaries, &lag.game, lag.after, &ctx);
                let whole = summary(&dir, &mut summaries, &lag.game, &ctx);
                let s = map_summary(league, &whole).unwrap_or_else(|e| panic!("{ctx}: {e}"));
                app.merge_summary(&lag.game, s);
                let cut = app
                    .cuts
                    .active(app.tick)
                    .unwrap_or_else(|| panic!("{ctx}: the retry landed and no cut is on screen"));
                assert_eq!(
                    cut.play.id, landed.id,
                    "{ctx}: the late cut names {:?}, not {:?}",
                    cut.play.text, landed.text
                );
                assert_eq!(app.cuts_fired(), before + 1, "{ctx}: exactly one cut");
                assert!(
                    app.catchup_wants().is_empty(),
                    "{ctx}: answered, so consumed"
                );
                println!(
                    "{label} poll {name}: {}-{} · retried after a lagging summary · {:?}",
                    lag.after.0, lag.after.1, cut.play.text
                );
            } else {
                assert!(
                    wants.is_empty(),
                    "{ctx}: a catch-up earlier than the drill's, which `first_catchup_delta` said was the first"
                );
            }
        }
        drilled += 1;
    }
    assert!(
        drilled > 0,
        "no sequence has a delta that takes the catch-up path: the retry is untested"
    );
}

#[test]
fn every_score_delta_in_every_sequence_yields_exactly_one_cut_naming_a_scoring_play() {
    for dir in sequences() {
        let league = league_of(&dir);
        let label = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut app = app(&label);
        let mut deltas = 0u32;
        let mut prev: HashMap<String, (u16, u16)> = Default::default();
        let mut summaries: HashMap<String, String> = Default::default();
        for (name, body) in polls(&dir) {
            let games = map_scoreboard(league, &body, time::UtcOffset::UTC)
                .unwrap_or_else(|e| panic!("{}: poll {name}: {e}", dir.display()));
            // Every game in this poll's payload that moved, not just the one
            // the sequence was captured for: the assertion below is per poll,
            // so a poll with two scores must fire two cuts.
            let mut moved: Vec<(String, (u16, u16))> = Vec::new();
            for g in &games {
                let score = (g.away_score, g.home_score);
                if prev.get(&g.id).is_some_and(|p| *p != score) {
                    deltas += 1;
                    moved.push((g.id.clone(), score));
                }
                prev.insert(g.id.clone(), score);
            }
            // The 15 s between polls, then the payload: a cut fired by this
            // poll is still on screen when the assertions read it, and one
            // fired by the last poll is long gone.
            for _ in 0..TICKS_PER_POLL {
                app.advance_tick();
            }
            let before = app.cuts_fired();
            app.apply_boards(league, games, false);
            // Serve every queued catch-up the captured summary WHOLE, exactly
            // as it sits on disk — runs from later innings included. A
            // production fetch would stop at the moment of the delta; this
            // one deliberately does not, so the cut below is proof that the
            // app picks by score and not by "the newest row I have not seen".
            let wants = app.catchup_wants();
            for c in &wants {
                let ctx = format!("{}: poll {name}: {}", dir.display(), c.game_id);
                let body = summary(&dir, &mut summaries, &c.game_id, &ctx);
                let s =
                    map_summary(c.league, &body).unwrap_or_else(|e| panic!("{ctx}: summary: {e}"));
                app.merge_summary(&c.game_id, s);
            }
            let fired = app.cuts_fired() - before;
            assert_eq!(
                fired as usize,
                moved.len(),
                "{}: poll {name}: {} score deltas but {fired} cuts",
                dir.display(),
                moved.len()
            );
            // One game moved: the cut on screen names the play that made the
            // score what this poll reports. Two games in one poll share the
            // one CutState slot, so the naming check is skipped there and the
            // count above carries the poll.
            if let [(id, score)] = moved.as_slice() {
                let ctx = format!("{}: poll {name}: {id}", dir.display());
                let want = run_for(&dir, &mut summaries, id, *score, &ctx);
                let cut = app.cuts.active(app.tick).unwrap_or_else(|| {
                    panic!(
                        "{ctx}: the score moved to {}-{} and no cut is on screen",
                        score.0, score.1
                    )
                });
                assert_eq!(
                    cut.play.id, want.id,
                    "{ctx}: the cut names {:?} ({}), but the run that made it {}-{} is {:?} ({})",
                    cut.play.text, cut.play.id, score.0, score.1, want.text, want.id
                );
                // The receipt the report reads: which path fired, and the words.
                let path = if wants.iter().any(|c| &c.game_id == id) {
                    "catch-up"
                } else {
                    "scoreboard lastPlay"
                };
                println!(
                    "{label} poll {name}: {}-{} · {path} · {:?}",
                    score.0, score.1, cut.play.text
                );
            }
        }
        assert!(
            deltas > 0,
            "{}: no score delta in this sequence; it does not earn its place",
            dir.display()
        );
        assert_eq!(
            app.cuts_fired(),
            deltas,
            "{}: {deltas} score deltas but {} cuts",
            dir.display(),
            app.cuts_fired()
        );
        assert!(
            app.catchup_wants().is_empty(),
            "{}: a catch-up was left pending",
            dir.display()
        );
        // Every captured scoring play is a real scoring play of the right
        // kind, credited to one of the two teams.
        for (game, play) in app.scoring_events() {
            assert!(play.scoring, "{}: {:?}", dir.display(), play.text);
            assert!(
                !play.id.is_empty(),
                "{}: a captured play without an id: {:?}",
                dir.display(),
                play.text
            );
            assert!(
                play.team == game.away.abbr || play.team == game.home.abbr,
                "{}: credited to {:?}, teams {} {}",
                dir.display(),
                play.team,
                game.away.abbr,
                game.home.abbr
            );
            let kinds_ok = match league {
                League::Mlb => matches!(play.kind, PlayKind::HomeRun | PlayKind::RunScoringPlay),
                League::Nfl | League::Cfb => matches!(
                    play.kind,
                    PlayKind::Touchdown | PlayKind::FieldGoal | PlayKind::Safety
                ),
                League::Nhl | League::Epl | League::Mls => matches!(
                    play.kind,
                    PlayKind::Goal | PlayKind::OwnGoal | PlayKind::PenaltyGoal
                ),
                _ => true,
            };
            assert!(
                kinds_ok,
                "{}: {:?} is not a scoring kind for {:?}: {:?}",
                dir.display(),
                play.kind,
                league,
                play.text
            );
        }
    }
}
