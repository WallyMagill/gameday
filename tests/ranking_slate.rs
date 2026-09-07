//! The Saturday that exposed R1: the v3.4 formula led with an FCS 0-17 game
//! carrying a 2-MIN chip while No. 2 Oregon trailed Boise State 7-17 at a 28%
//! home win probability. This test pins the wave-2 order on that exact slate.
use gameday::domain::*;
use gameday::provider::map::map_scoreboard;
use gameday::rank::{leverage_closeness, top_id, watchability, SortKey};

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
    let mut scored: Vec<(u32, &'static str, String, usize)> = live
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let w = watchability(g, now);
            (
                w.score,
                w.why,
                format!(
                    "{} {} @ {} {}",
                    g.away.abbr, g.away_score, g.home.abbr, g.home_score
                ),
                i,
            )
        })
        .collect();
    scored.sort_by_key(|b| std::cmp::Reverse(b.0));
    let top5: Vec<String> = scored
        .iter()
        .take(5)
        .map(|(s, w, n, _)| format!("{s:>3} {w:<9} {n}"))
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
        .position(|(_, _, n, _)| n.starts_with("FOR "))
        .unwrap();
    assert!(
        fcs_pos >= 3,
        "FOR at NDSU (0-17) sits at {fcs_pos}; top five:\n{}",
        top5.join("\n")
    );
    // Floor 40 is a guess with margin: on this slate the top three read
    // 57/76/77 and the blowouts read 0-12 (SEMO at ISU is 0 with 1:52 left;
    // FOR at NDSU is 9). A blowout riding a late chip into the top three is
    // exactly R1.
    for (_, _, name, idx) in scored.iter().take(3) {
        let g = &live[*idx];
        let win_prob = g
            .situation
            .as_ref()
            .and_then(|s| s.win_prob)
            .unwrap_or_else(|| panic!("top-three game {name} has no win_prob"));
        let c = leverage_closeness(&win_prob);
        assert!(c >= 40, "top-three game {name} has closeness {c}, floor 40");
    }
    println!("top five:\n{}", top5.join("\n"));
}
