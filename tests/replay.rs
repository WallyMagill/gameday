//! Replay of consecutive real scoreboard polls through the real apply path.
//! Every sequence under fixtures/replay/ is one live window captured by
//! scripts/capture-replay.sh. The assertion is the one the 2026-09-05 review
//! found broken on live data: when a score moves, the cut names the play
//! that scored — never the pitch or snap the poll happened to catch.
use gameday::app::App;
use gameday::config::Config;
use gameday::domain::*;
use gameday::provider::map::{map_scoreboard, map_summary};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn app(name: &str) -> App {
    let dir = std::env::temp_dir().join(format!("gd-replay-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut app = App::new(Config::default_all(), vec![], dir, time::UtcOffset::UTC);
    // Ten seconds of ticks per poll keeps the catch-up TTL honest: a poll is
    // 15 s live, and the TTL is four polls.
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
/// polling when one fetch fails, so `07.json` may simply not exist and an
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

/// The captured summary as it stood when the score was `(away, home)`, plus
/// the id and text of the play that made it so.
struct AsOf {
    body: String,
    id: String,
    text: String,
}

/// FINDING (2026-09-07, this harness's first run). The summary in each
/// sequence is fetched once, after the last poll, so it carries every scoring
/// play of the whole window. Served whole to the first catch-up, the app's
/// "newest unseen scoring play" is the LAST run of the window: served whole,
/// poll 19 of mlb-20260907-0334 (4-2, bottom 5th) fires a cut naming
/// "T. Hernández reached on infield single…", the 4-4 that had not happened
/// yet. It poisons the next delta too — poll 40's scoreboard play is already
/// in the list that summary backfilled, so the 4-3 fires no cut at all
/// (`poll 40.json: 1 score deltas but 0 cuts`). Production never sees either:
/// the summary is fetched at delta time, so it stops at the play that just
/// scored. So the harness reconstructs that fetch rather than replaying a
/// fixture the app could never have received.
///
/// The cutoff is the SCORE, not `situation.lastPlay.id`. ESPN's scoreboard
/// moves the score before it moves its last play: poll 19 of
/// mlb-20260907-0334 already reports 4-2 while `lastPlay` is still
/// `4018168410903020005` "Pitch 1 : Ball 1" — two rows BEFORE
/// `4018168410903990057` "Edman doubled to left, Muncy scored.", the run that
/// made it 4-2 (that lag is the whole reason the catch-up path exists). An
/// id ≤ lastPlay.id cutoff would drop the very play the cut must name and
/// hand the app the top of the 4th instead. Post-play `awayScore`/`homeScore`
/// is the honest witness: keep the feed through the last scoring row whose
/// resulting score is this poll's score. Conservative by construction —
/// nothing that happened after the run this delta reports can survive it.
///
/// Flat arrays only (`plays` for MLB/NBA/NHL, `scoringPlays` for football).
/// A football sequence would also want `drives.previous[].plays` truncated —
/// nothing reads those for the cut, so that is left until a football window
/// is captured.
fn summary_as_of(body: &str, (away, home): (u16, u16), ctx: &str) -> AsOf {
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
    AsOf {
        body: v.to_string(),
        id,
        text,
    }
}

/// `summary_as_of` for `game_id`, reading each sequence's summary from disk
/// once (they run to megabytes).
fn as_of(
    dir: &Path,
    cache: &mut HashMap<String, String>,
    game_id: &str,
    score: (u16, u16),
    ctx: &str,
) -> AsOf {
    let raw = cache.entry(game_id.to_string()).or_insert_with(|| {
        summary_for(dir, game_id).unwrap_or_else(|| {
            panic!("{ctx}: the score moved but no summary-{game_id}.json was captured")
        })
    });
    summary_as_of(raw, score, ctx)
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
            let before = app.cuts_fired();
            app.apply_boards(league, games, false);
            // Ten render ticks per poll so the catch-up TTL is measured in polls.
            for _ in 0..10 {
                app.advance_tick();
            }
            // Serve every queued catch-up from the captured summary, as the
            // poll thread would — truncated to the moment of this poll, which
            // is what the poll thread's own fetch would have returned.
            let wants = app.catchup_wants();
            for c in &wants {
                let score = *prev.get(&c.game_id).unwrap_or_else(|| {
                    panic!(
                        "{}: poll {name}: a catch-up for {} that is on no board",
                        dir.display(),
                        c.game_id
                    )
                });
                let ctx = format!("{}: poll {name}: {}", dir.display(), c.game_id);
                let served = as_of(&dir, &mut summaries, &c.game_id, score, &ctx);
                let s = map_summary(c.league, &served.body)
                    .unwrap_or_else(|e| panic!("{ctx}: summary: {e}"));
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
                let want = as_of(&dir, &mut summaries, id, *score, &ctx);
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
