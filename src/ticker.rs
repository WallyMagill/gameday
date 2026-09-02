//! Bottom-of-board ticker in the ESPN BottomLine shape: a thin rule, then two
//! unboxed lanes. SCORES is every live game across the enabled leagues as
//! whole `NFL KC 27 TB 24 Q4 1:27` segments — games that don't fit rotate in,
//! never a mid-game cut. ALERTS is every scoring play; it marquees when it
//! overflows so each event eventually comes into view.

use crate::domain::{Game, Play};
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub type Cells = Vec<(char, Style)>;

/// Rows the ticker takes: the rule plus one row per lane.
pub const HEIGHT: u16 = 3;

/// Lane gutter labels; the only chrome besides the rule.
const LANE_LABELS: [&str; 2] = [" SCORES ", " ALERTS "];

/// Ticks each rotation of the scores lane holds before the next game steps
/// in. A guess: 30 ticks is ~3 s at the live loop's ~10 fps, long enough to
/// read a score strip; nothing measured yet.
const SCORES_DWELL_TICKS: u64 = 30;

/// Blank cells between the tail and the wrapped head of a scrolling lane —
/// enough of a gap to read as "the reel restarted".
const MARQUEE_GAP: usize = 10;

/// Rule, SCORES lane, ALERTS lane. `live` is every live game the board
/// knows about (not just the visible tab — the ticker's job is the games
/// you are *not* looking at); `events` is every scoring play among them.
pub fn draw(frame: &mut Frame, area: Rect, live: &[Game], events: &[(Game, Play)], tick: u64) {
    let th = theme::current();
    let ground = Style::default().bg(th.bg);
    let sep = Style::default().fg(th.dim);
    let gutter = LANE_LABELS[0].chars().count();
    let content_w = (area.width as usize).saturating_sub(gutter);

    let segments: Vec<Cells> = live.iter().map(score_segment).collect();
    let lanes = [
        whole_segments(&segments, content_w, tick / SCORES_DWELL_TICKS, sep),
        alerts_lane(events),
    ];

    let mut lines = vec![Line::from(Span::styled("─".repeat(area.width as usize), sep))];
    for (label, lane) in LANE_LABELS.iter().zip(&lanes) {
        let mut spans = vec![Span::styled(*label, Style::default().fg(th.muted))];
        spans.extend(marquee_spans(lane, content_w, tick));
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines).style(ground), area);
}

/// One game on the scores lane: `NFL KC 27 TB 24 Q4 1:27`.
pub fn score_segment(g: &Game) -> Cells {
    let th = theme::current();
    let mut seg = Cells::new();
    push(&mut seg, &g.league.slug().to_uppercase(), Style::default().fg(th.chip(g.league)).add_modifier(Modifier::BOLD));
    push(&mut seg, &format!(" {} ", g.away.abbr), Style::default().fg(th.fg));
    push(&mut seg, &g.away_score.to_string(), Style::default().fg(th.bright).add_modifier(Modifier::BOLD));
    push(&mut seg, &format!(" {} ", g.home.abbr), Style::default().fg(th.fg));
    push(&mut seg, &g.home_score.to_string(), Style::default().fg(th.bright).add_modifier(Modifier::BOLD));
    let when = format!("{} {}", g.period, g.clock);
    push(&mut seg, &format!(" {}", when.trim()), Style::default().fg(th.clock()));
    seg
}

/// As many whole `segments` as fit in `width`, `│`-separated, starting from
/// segment `step % n` and wrapping — so the visible lane is always complete
/// games. Fits-all case: every segment, from the first, at every step.
pub fn whole_segments(segments: &[Cells], width: usize, step: u64, sep: Style) -> Cells {
    let n = segments.len();
    let mut out = Cells::new();
    if n == 0 {
        return out;
    }
    let sep_cells: Cells = " │ ".chars().map(|c| (c, sep)).collect();
    let total: usize = segments.iter().map(Vec::len).sum::<usize>() + sep_cells.len() * (n - 1);
    let start = if total <= width { 0 } else { (step as usize) % n };
    for k in 0..n {
        let seg = &segments[(start + k) % n];
        let need = seg.len() + if out.is_empty() { 0 } else { sep_cells.len() };
        if out.len() + need > width {
            break;
        }
        if !out.is_empty() {
            out.extend(sep_cells.iter().copied());
        }
        out.extend(seg.iter().copied());
    }
    out
}

/// Rows the off-screen SCORES lane costs on its own — no rule, no ALERTS.
pub const LANE_HEIGHT: u16 = 1;

/// The SCORES lane alone, gutter + whole score segments, no rule and no
/// ALERTS lane. The Board draws its own inline off-screen lane
/// (`board::mod::draw_lane`) straight into its body and never allocates
/// these rows (spec §1: one lane, one owner) — this is what `App::draw`
/// gives every OTHER view instead, gated on whether the Board's own
/// `layout::plan(...).scores_lane` says the list would truncate at the
/// current size, so a viewer parked in Zoom/Standings/the plays feed still
/// sees what is off the Board without a second ticker grammar.
pub fn draw_lane(frame: &mut Frame, area: Rect, live: &[Game], tick: u64) {
    let th = theme::current();
    let sep = Style::default().fg(th.dim);
    let gutter = LANE_LABELS[0];
    let content_w = (area.width as usize).saturating_sub(gutter.chars().count());
    let segments: Vec<Cells> = live.iter().map(score_segment).collect();
    let lane = whole_segments(&segments, content_w, tick / SCORES_DWELL_TICKS, sep);
    let mut spans = vec![Span::styled(gutter, Style::default().fg(th.muted))];
    spans.extend(group_spans(lane.into_iter()));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// Every scoring event as `1:27 KC TD text 27-24 KC`, `│`-separated. No cap:
/// the marquee brings each into view.
pub fn alerts_lane(events: &[(Game, Play)]) -> Cells {
    let th = theme::current();
    let mut lane = Cells::new();
    if events.is_empty() {
        push(&mut lane, "no scoring plays yet", Style::default().fg(th.dim));
        return lane;
    }
    for (game, play) in events {
        if !lane.is_empty() {
            push(&mut lane, " │ ", Style::default().fg(th.dim));
        }
        push(&mut lane, &format!("{} ", play.clock), Style::default().fg(th.clock()));
        push(&mut lane, &format!("{} ", play.team), Style::default().fg(crate::app::App::team_color(game, &play.team)).add_modifier(Modifier::BOLD));
        push(&mut lane, &format!("{} ", theme::scoring_word(game.league)), Style::default().fg(th.live).add_modifier(Modifier::BOLD));
        push(&mut lane, &play.text, Style::default().fg(th.fg));
        push(&mut lane, &format!(" {}", leader_score(game)), Style::default().fg(th.bright));
    }
    lane
}

/// `LEAD-TRAIL LDR`: the leader's score first, then who leads.
fn leader_score(g: &Game) -> String {
    if g.away_score >= g.home_score {
        format!("{}-{} {}", g.away_score, g.home_score, g.away.abbr)
    } else {
        format!("{}-{} {}", g.home_score, g.away_score, g.home.abbr)
    }
}

fn push(row: &mut Cells, text: &str, style: Style) {
    row.extend(text.chars().map(|c| (c, style)));
}

/// A `width`-cell window into `cells`, scrolled one cell per render tick with
/// wraparound. Content that fits renders unshifted — no motion. Pure in
/// (cells, width, tick).
fn marquee_spans(cells: &[(char, Style)], width: usize, tick: u64) -> Vec<Span<'static>> {
    if cells.len() <= width {
        return group_spans(cells.iter().copied());
    }
    let total = cells.len() + MARQUEE_GAP;
    let offset = (tick as usize) % total;
    group_spans((0..width).map(|i| {
        let idx = (offset + i) % total;
        cells.get(idx).copied().unwrap_or((' ', Style::default()))
    }))
}

/// Merge runs of identically-styled cells back into spans.
fn group_spans(cells: impl Iterator<Item = (char, Style)>) -> Vec<Span<'static>> {
    let mut out: Vec<(String, Style)> = Vec::new();
    for (ch, style) in cells {
        match out.last_mut() {
            Some((text, last)) if *last == style => text.push(ch),
            _ => out.push((ch.to_string(), style)),
        }
    }
    out.into_iter().map(|(text, style)| Span::styled(text, style)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use ratatui::style::Style;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn team(abbr: &str) -> Team {
        Team {
            id: abbr.into(),
            abbr: abbr.into(),
            name: abbr.into(),
            color: [1, 2, 3],
            logo_key: format!("nfl/{}", abbr.to_lowercase()),
            ..Default::default()
        }
    }

    fn game(league: League, away: &str, home: &str, scores: (u16, u16), when: (&str, &str)) -> Game {
        Game {
            id: format!("{away}-{home}"),
            league,
            away: team(away),
            home: team(home),
            away_score: scores.0,
            home_score: scores.1,
            status: Status::Live,
            period: when.0.into(),
            clock: when.1.into(),
            situation: None,
            last_plays: vec![],
            meter: None,
            ..Game::default()
        }
    }

    fn text(cells: &[(char, Style)]) -> String {
        cells.iter().map(|(c, _)| *c).collect()
    }

    fn cells(s: &str) -> Cells {
        s.chars().map(|c| (c, Style::default())).collect()
    }

    fn window_text(cells: &[(char, Style)], width: usize, tick: u64) -> String {
        marquee_spans(cells, width, tick)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn score_segment_is_league_teams_scores_and_clock() {
        let g = game(League::Nfl, "KC", "TB", (27, 24), ("Q4", "1:27"));
        assert_eq!(text(&score_segment(&g)), "NFL KC 27 TB 24 Q4 1:27");
    }

    #[test]
    fn score_segment_drops_a_blank_clock() {
        // Soccer periods carry no clock; the segment must not end in a space.
        let g = game(League::Epl, "ARS", "CHE", (1, 0), ("2H", ""));
        assert_eq!(text(&score_segment(&g)), "EPL ARS 1 CHE 0 2H");
    }

    #[test]
    fn scores_lane_shows_only_whole_games_and_rotates_when_they_overflow() {
        let segs = vec![cells("AAAA"), cells("BBBB"), cells("CCCC")];
        let sep = Style::default();
        assert_eq!(text(&whole_segments(&segs, 11, 0, sep)), "AAAA │ BBBB");
        assert_eq!(text(&whole_segments(&segs, 11, 1, sep)), "BBBB │ CCCC");
        assert_eq!(text(&whole_segments(&segs, 11, 2, sep)), "CCCC │ AAAA");
        assert_eq!(text(&whole_segments(&segs, 11, 3, sep)), "AAAA │ BBBB", "wraps");
        assert_eq!(text(&whole_segments(&segs, 10, 0, sep)), "AAAA", "never a cut game");
        assert_eq!(
            text(&whole_segments(&segs, 40, 7, sep)),
            "AAAA │ BBBB │ CCCC",
            "fits-all: every game, from the first, at every step"
        );
    }

    #[test]
    fn alerts_lane_carries_every_scoring_event() {
        let mut g = game(League::Nfl, "KC", "TB", (27, 24), ("Q4", "1:27"));
        g.last_plays = (0..8)
            .map(|i| Play {
                clock: format!("{i}:00"),
                team: "KC".into(),
                text: format!("score number {i}"),
                scoring: true,
                ..Default::default()
            })
            .collect();
        let events: Vec<(Game, Play)> = g.last_plays.iter().map(|p| (g.clone(), p.clone())).collect();
        let lane = text(&alerts_lane(&events));
        for i in 0..8 {
            let needle = format!("{i}:00 KC TOUCHDOWN! score number {i} 27-24 KC");
            assert!(lane.contains(&needle), "missing {needle:?} in {lane:?}");
        }
    }

    #[test]
    fn empty_alerts_lane_says_so() {
        assert_eq!(text(&alerts_lane(&[])), "no scoring plays yet");
    }

    #[test]
    fn marquee_is_static_when_content_fits() {
        let c = cells("SHORT");
        assert_eq!(window_text(&c, 10, 0), "SHORT");
        assert_eq!(window_text(&c, 10, 7), "SHORT", "no motion when it fits");
    }

    #[test]
    fn marquee_scrolls_one_cell_per_tick_and_wraps() {
        let c = cells("ABCDEFGHIJ"); // 10 cells, window 6, cycle 10+GAP=20
        assert_eq!(window_text(&c, 6, 0), "ABCDEF");
        assert_eq!(window_text(&c, 6, 1), "BCDEFG");
        assert_eq!(window_text(&c, 6, 4), "EFGHIJ", "tail scrolls into view");
        assert_eq!(window_text(&c, 6, 15), "     A", "gap, then the head wraps");
        assert_eq!(window_text(&c, 6, 20), "ABCDEF", "full cycle");
    }

    #[test]
    fn draw_is_a_rule_then_a_scores_lane_then_an_alerts_lane() {
        let live = vec![
            game(League::Nfl, "KC", "TB", (27, 24), ("Q4", "1:27")),
            game(League::Nba, "DEN", "BOS", (88, 81), ("Q3", "4:38")),
        ];
        let mut term = Terminal::new(TestBackend::new(80, HEIGHT)).unwrap();
        term.draw(|f| draw(f, f.area(), &live, &[], 0)).unwrap();
        let buf = term.backend().buffer();
        let row = |y: u16| -> String { (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect() };
        assert_eq!(row(0), "─".repeat(80), "row 0 is the rule");
        assert!(row(1).starts_with(" SCORES NFL KC 27 TB 24 Q4 1:27 │ NBA DEN 88 BOS 81 Q3 4:38"), "{:?}", row(1));
        assert!(row(2).starts_with(" ALERTS no scoring plays yet"), "{:?}", row(2));
    }

    #[test]
    fn lane_is_one_row_gutter_and_scores_no_rule_no_alerts() {
        // Task 9: what non-Board views get instead of the old rule+2-lane
        // ticker — the gutter is bright cyan/muted like `draw`'s, but there
        // is exactly one row, and no ALERTS lane at all.
        let live = vec![game(League::Nfl, "KC", "TB", (27, 24), ("Q4", "1:27"))];
        let mut term = Terminal::new(TestBackend::new(80, LANE_HEIGHT)).unwrap();
        term.draw(|f| draw_lane(f, f.area(), &live, 0)).unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..80).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row.trim_end(), " SCORES NFL KC 27 TB 24 Q4 1:27", "{row:?}");
        assert_eq!(buf[(1, 0)].fg, theme::current().muted, "the gutter takes the muted role");
    }
}
