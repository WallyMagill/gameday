//! Big-glyph, meter and play-row helpers — what is left of the tile grammar.
//!
//! v3.2 §7 deleted the mosaic; Task 13 deleted the last tile with it (the zoom
//! Overview is now the hero block, spec §5). No renderer here draws a whole
//! surface any more: `glyph_cell`/`digit_glyphs`/`word_glyphs` are the one
//! glyph engine the hero, the cut, `:tv` and the tier rows all share,
//! `meter_line` is the inline gauge the hero and the zoom draw, and
//! `play_stamp`/`play_line` format one play row for the zoom's feed.

pub mod quad_digits;

use crate::domain::{Game, Meter};
use crate::text::truncate;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;

/// Narrowest track the RED ZONE / LEAD gauges draw: at 8 cells the red-zone
/// marker moves every ~2.5 yards and a ±2 lead visibly leaves center. A
/// by-eye pick, not a measurement — the row's value tag shortens before the
/// track is allowed to drop below it.
const METER_MIN_BAR: usize = 8;
/// Longest RED ZONE / LEAD track. At the zoom view's 100+ columns a 20-yard
/// gauge stops reading as a gauge (and sat on top of the full-width drive bar
/// as a near-duplicate). By eye.
const METER_MAX_BAR: usize = 40;
/// Longest penalty countdown bar: past 20 cells (6 s per cell for a minor)
/// the drain reads as a progress bar rather than a clock. By eye.
const PENALTY_BAR_MAX: usize = 20;
/// The minor penalty the countdown bar is scaled to (2:00 by rule; a major
/// shows as a full bar until it is inside its last two minutes).
const PENALTY_MINOR_SECS: u16 = 120;

/// Cell size of one big-text glyph: 8×8, `PixelSize::Full`. The one place
/// those numbers are written down — the hero's fit ladder and the cut's word
/// ladder both step through them.
///
/// There is no second size here any more. `PixelSize::Sextant` was 4×3 and
/// drew from U+1FB00–1FB3B, which Terminal.app's default font does not cover;
/// sitting-1 pick 1A replaced the digits' sextant rung with
/// [`quad_digits`] and ruling R42 deleted the scoring word's. What used to
/// step down in size now steps down in *kind* — to a plain bold line.
pub(crate) fn glyph_cell() -> (u16, u16) {
    GLYPH_CELL
}

/// [`glyph_cell`] as a constant, for the callers that need the number in a
/// `const` (the hero's jumbotron gate is `GLYPH_CELL.1 * 2`).
pub(crate) const GLYPH_CELL: (u16, u16) = (8, 8);

/// Paint `text` as big glyphs into `rect` in `style`. The rect is the
/// caller's clamped slot — the widget clips, this never grows it.
fn glyph_slot(frame: &mut Frame, rect: Rect, text: &str, style: Style) {
    use tui_big_text::{BigText, PixelSize};
    frame.render_widget(
        BigText::builder()
            .pixel_size(PixelSize::Full)
            .style(style)
            .lines(vec![Line::from(text.to_string())])
            .build(),
        rect,
    );
}

/// Big letters for a word (the cut's scoring word), in `rect`, at the size
/// the caller already measured with [`glyph_cell`]. Same renderer as the
/// digits' Full rung — one glyph engine, so a word and a score never disagree
/// about their cell grid.
pub(crate) fn word_glyphs(frame: &mut Frame, rect: Rect, word: &str, color: Color) {
    glyph_slot(frame, rect, word, Style::default().fg(color));
}

/// One number, one color, one rect at `PixelSize::Full`: the big-score core.
/// Returns false — drawing nothing — when the glyphs don't fit `rect`, which
/// is how every caller steps down a size instead of clipping a digit in half.
///
/// There is no sextant arm any more (sitting-1 pick 1A): the mid rung of the
/// score ladder is [`quad_digits`], and the tier-1 sextant garnish that was
/// this function's only other small-form caller is deleted (ruling R39).
pub(crate) fn digit_glyphs(frame: &mut Frame, rect: Rect, value: u16, color: Color) -> bool {
    let text = value.to_string();
    let (gw, gh) = glyph_cell();
    if text.len() as u16 * gw > rect.width || gh > rect.height {
        return false;
    }
    glyph_slot(frame, rect, &text, Style::default().fg(color));
    true
}

/// The bracket stamp on a play row: the game clock when the sport has one,
/// otherwise the play's period (baseball `B9`). `-:--` only when the feed
/// gave us neither.
pub(crate) fn play_stamp(p: &crate::domain::Play) -> &str {
    if !p.clock.is_empty() {
        &p.clock
    } else if !p.period.is_empty() {
        &p.period
    } else {
        "-:--"
    }
}

/// `[clock] ABB text` row for one play, truncated to `width`.
pub(crate) fn play_line(game: &Game, p: &crate::domain::Play, width: usize) -> Line<'static> {
    let th = theme::current();
    // Team color on the abbr is a discipline grant, not a given.
    let team_color = if p.team.eq_ignore_ascii_case(&game.away.abbr) {
        th.team_text(game.away.color)
    } else if p.team.eq_ignore_ascii_case(&game.home.abbr) {
        th.team_text(game.home.color)
    } else {
        th.fg
    };
    let clock = format!(" [{}]", play_stamp(p));
    let abbr = format!(" {:<3} ", p.team);
    let used = clock.chars().count() + abbr.chars().count();
    let text = truncate(&p.text, width.saturating_sub(used + 1));
    Line::from(vec![
        Span::styled(clock, Style::default().fg(th.muted)),
        Span::styled(abbr, Style::default().fg(team_color).add_modifier(Modifier::BOLD)),
        Span::styled(text, Style::default().fg(th.fg)),
    ])
}

/// Meter B: the one-row inline gauge under the identity block. `None` when
/// the game carries no meter (soccer, pre/final, no data) — the caller gives
/// the row to the plays feed. Every row is `LABEL  track  VALUE`, left
/// aligned with the ` LAST PLAYS` caption; labels take the section-label
/// discipline, RED ZONE stays the earned live red, and the value tag
/// shortens rather than let the track fall under [`METER_MIN_BAR`] or the
/// row clip at the border.
pub(crate) fn meter_line(game: &Game, width: usize) -> Option<Line<'static>> {
    let th = theme::current();
    let meter = game.meter.as_ref()?;
    let label_style = Style::default()
        .fg(th.section_label(th.league_accent(game.league)))
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(th.muted);
    let dim = Style::default().fg(th.dim);
    let bright = Style::default().fg(th.bright).add_modifier(Modifier::BOLD);
    let live = Style::default().fg(th.live);
    let live_bold = live.add_modifier(Modifier::BOLD);
    let width_of = |spans: &[Span]| -> usize { spans.iter().map(|s| s.width()).sum() };
    let spans: Vec<Span<'static>> = match meter {
        Meter::RedZone { yards_to_goal } => {
            // Track runs the 20 → G, marker at the ball.
            let ytg = usize::from(*yards_to_goal).min(20);
            let label = " RED ZONE  ";
            let goal = "  G";
            let long = format!("   {ytg} TO GOAL");
            let short = format!("  {ytg} YD");
            let fixed = label.len() + goal.len();
            let value = if width >= fixed + long.len() + METER_MIN_BAR { long } else { short };
            let bar_w = width.saturating_sub(fixed + value.len()).clamp(2, METER_MAX_BAR);
            let filled = (bar_w - 1) * (20 - ytg) / 20;
            vec![
                Span::styled(label, live_bold),
                Span::styled("━".repeat(filled), live),
                Span::styled("●", live_bold),
                Span::styled("─".repeat(bar_w - 1 - filled), dim),
                Span::styled(goal, muted),
                Span::styled(value, bright),
            ]
        }
        Meter::Lead { plus_minus } => {
            // plus_minus is home − away: away on the left end (like the
            // identity block), home on the right. The ends are labeled with
            // the abbrs, not ±15: a signed scale put "DEN +7" on the minus
            // half whenever the away team led, which read as a bug. Marker
            // and tag wear the leader's color — the identity floor, not a
            // discipline grant.
            let pm = i32::from(*plus_minus);
            let (tag, tag_style) = match pm.signum() {
                0 => ("TIED".to_string(), bright),
                s => {
                    let team = if s < 0 { &game.away } else { &game.home };
                    (
                        format!("{} {:+}", team.abbr, pm.abs()),
                        Style::default().fg(theme::rgb(team.color)).add_modifier(Modifier::BOLD),
                    )
                }
            };
            let marker_style = if pm == 0 { muted } else { tag_style };
            let (end_lo, end_hi) = (format!("{} ", game.away.abbr), format!(" {}", game.home.abbr));
            let full = width
                >= " LEAD  ".len() + end_lo.len() + end_hi.len() + "   ".len() + tag.len() + METER_MIN_BAR;
            let (head, scale_lo, scale_hi, gap) = if full {
                (" LEAD  ", end_lo, end_hi, "   ")
            } else {
                (" LEAD ", String::new(), String::new(), "  ")
            };
            let bar_w = width
                .saturating_sub(head.len() + scale_lo.len() + scale_hi.len() + gap.len() + tag.len())
                .clamp(3, METER_MAX_BAR);
            // Rounded so a ±1 lead already steps off the center tick.
            let cell = |v: i32| ((v + 15) as usize * (bar_w - 1) * 2 + 30) / 60;
            let center = cell(0);
            let pos = cell(pm.clamp(-15, 15));
            let mut spans = vec![Span::styled(head, label_style), Span::styled(scale_lo, muted)];
            for i in 0..bar_w {
                spans.push(if i == pos {
                    Span::styled("▮", marker_style)
                } else if i == center {
                    Span::styled("┼", muted)
                } else {
                    Span::styled("─", dim)
                });
            }
            spans.push(Span::styled(scale_hi, muted));
            spans.push(Span::styled(gap, muted));
            spans.push(Span::styled(tag, tag_style));
            spans
        }
        Meter::Diamond { occupied } => {
            let sit = game.situation.as_ref();
            let outs = sit.and_then(|s| s.outs);
            let count = sit.and_then(|s| Some((s.balls?, s.strikes?)));
            let build = |full: bool| -> Vec<Span<'static>> {
                let gap = if full { "   " } else { "  " };
                let mut spans = vec![Span::styled(if full { " BASES  " } else { " BASES " }, label_style)];
                // First, second, third — left to right. Empty bases and
                // outs are information, so they sit at `muted`, not `dim`
                // (dim vanished on the tinted community palettes).
                for on in occupied {
                    spans.push(if *on {
                        Span::styled("◆", Style::default().fg(th.star).add_modifier(Modifier::BOLD))
                    } else {
                        Span::styled("◇", muted)
                    });
                }
                if let Some(o) = outs {
                    spans.push(Span::styled(format!("{gap}OUTS "), label_style));
                    for i in 0..3u8 {
                        spans.push(if i < o { Span::styled("●", bright) } else { Span::styled("○", muted) });
                    }
                }
                if let Some((b, s)) = count {
                    if full {
                        spans.push(Span::styled(format!("{gap}COUNT "), label_style));
                    } else {
                        spans.push(Span::styled(gap, muted));
                    }
                    spans.push(Span::styled(format!("{b}-{s}"), bright));
                }
                spans
            };
            let full = build(true);
            if width_of(&full) <= width { full } else { build(false) }
        }
        Meter::Penalty { team_abbr, seconds } => {
            // Countdown bar of a 2:00 minor: filled cells are the time left.
            let label = " PENALTY  ";
            let abbr = format!("{team_abbr} ");
            let clock = format!(" {}:{:02}", seconds / 60, seconds % 60);
            let bar_w = width
                .saturating_sub(label.len() + abbr.len() + clock.len())
                .clamp(1, PENALTY_BAR_MAX);
            let left = usize::from((*seconds).min(PENALTY_MINOR_SECS));
            let minor = usize::from(PENALTY_MINOR_SECS);
            // Ceiling: one second left still shows one cell.
            let filled = (left * bar_w).div_ceil(minor);
            vec![
                Span::styled(label, label_style),
                Span::styled(abbr, bright),
                Span::styled("▮".repeat(filled), live),
                Span::styled("░".repeat(bar_w - filled), dim),
                Span::styled(clock, live_bold),
            ]
        }
    };
    Some(Line::from(spans))
}
