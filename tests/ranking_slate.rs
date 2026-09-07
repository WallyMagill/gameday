//! The Saturday that exposed R1: the v3.4 formula led with an FCS 0-17 game
//! carrying a 2-MIN chip while No. 2 Oregon trailed Boise State 7-17 at a 28%
//! home win probability. This test pins the wave-2 order on that exact slate.
use gameday::domain::*;
use gameday::provider::map::map_scoreboard;
use gameday::rank::{top_id, watchability, SortKey};

#[test]
fn the_review_day_slate_leads_with_boise_at_oregon() {
    let games = map_scoreboard(
        League::Cfb,
        include_str!("../fixtures/review-slate-2026-09-05.json"),
        time::UtcOffset::from_hms(-4, 0, 0).unwrap(),
    )
    .unwrap();
    let live: Vec<Game> = games
        .into_iter()
        .filter(|g| g.status == Status::Live)
        .collect();
    assert_eq!(live.len(), 18);
    let now = time::OffsetDateTime::now_utc();
    let mut scored: Vec<(u32, &'static str, String)> = live
        .iter()
        .map(|g| {
            let w = watchability(g, now);
            (
                w.score,
                w.why,
                format!(
                    "{} {} @ {} {}",
                    g.away.abbr, g.away_score, g.home.abbr, g.home_score
                ),
            )
        })
        .collect();
    scored.sort_by_key(|b| std::cmp::Reverse(b.0));
    let top5: Vec<String> = scored
        .iter()
        .take(5)
        .map(|(s, w, n)| format!("{s:>3} {w:<9} {n}"))
        .collect();
    assert!(
        scored[0].2.starts_with("BOIS "),
        "top five:\n{}",
        top5.join("\n")
    );
    assert_eq!(
        top_id(&live, SortKey::Watch, &[League::Cfb], now)
            .as_deref()
            .map(|id| live.iter().find(|g| g.id == id).unwrap().away.abbr.as_str()),
        Some("BOIS")
    );
    let fcs_pos = scored
        .iter()
        .position(|(_, _, n)| n.starts_with("FOR "))
        .unwrap();
    assert!(
        fcs_pos >= 3,
        "FOR at NDSU (0-17) sits at {fcs_pos}; top five:\n{}",
        top5.join("\n")
    );
    // Nothing with a win probability leads on lateness alone.
    for g in &live {
        if g.situation.as_ref().and_then(|s| s.win_prob).is_some() {
            assert_ne!(watchability(g, now).why, "LATE", "{}", g.away.abbr);
        }
    }
    println!("top five:\n{}", top5.join("\n"));
}
