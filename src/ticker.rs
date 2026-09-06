//! The off-screen SCORES lane every non-Board view carries: one unboxed row
//! of whole `NFL KC 27 TB 24 Q4 1:27` segments — games that don't fit rotate
//! in, never a mid-game cut.
//!
//! The older boxed rule + SCORES + ALERTS ticker is deleted: the Board draws
//! its own inline lane (`board::draw_lane`) and scoring plays are the cut's
//! job, so the marquee, the ALERTS lane and the three-row `draw` had no
//! callers left.

use crate::domain::Game;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub type Cells = Vec<(char, Style)>;

/// The lane's gutter label; its only chrome.
const LANE_LABEL: &str = " SCORES ";

/// Ticks each rotation of the scores lane holds before the next game steps
/// in. A guess: 30 ticks is ~3 s at the live loop's ~10 fps, long enough to
/// read a score strip; nothing measured yet.
const SCORES_DWELL_TICKS: u64 = 30;

/// One game on the scores lane: `NFL KC 27 TB 24 Q4 1:27`.
pub fn score_segment(g: &Game) -> Cells {
    let th = theme::current();
    let mut seg = Cells::new();
    push(
        &mut seg,
        &g.league.slug().to_uppercase(),
        Style::default()
            .fg(th.chip(g.league))
            .add_modifier(Modifier::BOLD),
    );
    push(
        &mut seg,
        &format!(" {} ", g.away.abbr),
        Style::default().fg(th.fg),
    );
    push(
        &mut seg,
        &g.away_score.to_string(),
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
    );
    push(
        &mut seg,
        &format!(" {} ", g.home.abbr),
        Style::default().fg(th.fg),
    );
    push(
        &mut seg,
        &g.home_score.to_string(),
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
    );
    let when = format!("{} {}", g.period, g.clock);
    push(
        &mut seg,
        &format!(" {}", when.trim()),
        Style::default().fg(th.clock()),
    );
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
    let start = if total <= width {
        0
    } else {
        (step as usize) % n
    };
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
/// these rows — one lane, one owner — this is what `App::draw`
/// gives every OTHER view instead, gated on whether the Board's own
/// `layout::plan(...).scores_lane` says the list would truncate at the
/// current size, so a viewer parked in Zoom/Standings/the plays feed still
/// sees what is off the Board without a second ticker grammar.
pub fn draw_lane(frame: &mut Frame, area: Rect, live: &[Game], tick: u64) {
    let th = theme::current();
    let sep = Style::default().fg(th.dim);
    let gutter = LANE_LABEL;
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

fn push(row: &mut Cells, text: &str, style: Style) {
    row.extend(text.chars().map(|c| (c, style)));
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
    out.into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::*;
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;
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

    fn game(
        league: League,
        away: &str,
        home: &str,
        scores: (u16, u16),
        when: (&str, &str),
    ) -> Game {
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
        assert_eq!(
            text(&whole_segments(&segs, 11, 3, sep)),
            "AAAA │ BBBB",
            "wraps"
        );
        assert_eq!(
            text(&whole_segments(&segs, 10, 0, sep)),
            "AAAA",
            "never a cut game"
        );
        assert_eq!(
            text(&whole_segments(&segs, 40, 7, sep)),
            "AAAA │ BBBB │ CCCC",
            "fits-all: every game, from the first, at every step"
        );
    }

    #[test]
    fn lane_is_one_row_gutter_and_scores_no_rule_no_alerts() {
        // What non-Board views get instead of the old rule+2-lane
        // ticker — the gutter is bright cyan/muted like `draw`'s, but there
        // is exactly one row, and no ALERTS lane at all.
        let live = vec![game(League::Nfl, "KC", "TB", (27, 24), ("Q4", "1:27"))];
        let mut term = Terminal::new(TestBackend::new(80, LANE_HEIGHT)).unwrap();
        term.draw(|f| draw_lane(f, f.area(), &live, 0)).unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..80).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row.trim_end(), " SCORES NFL KC 27 TB 24 Q4 1:27", "{row:?}");
        assert_eq!(
            buf[(1, 0)].fg,
            theme::current().muted,
            "the gutter takes the muted role"
        );
    }
}
