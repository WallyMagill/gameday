//! Shared chrome: the header row, the ticker strip, the footer row and the
//! '?' overlay. Identical across every view — `views/` draws only the body
//! between them. Lifted out of `app.rs` whole (Task 16); the only edits are
//! the three reads that now come from the frame's `Derived` instead of
//! re-deriving their own lists.

use super::{date_label, App, Tab};
use crate::app::net::NetChip;
use crate::input::InputMode;
use crate::keymap;
use crate::theme;
use crate::ticker;
use crate::views::View;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::ops::Range;
use std::time::Instant;

/// One rung of the header's shed ladder — how much of the header's LEFT side
/// survives at this level of narrowness. Rungs are ordered most generous
/// first and give up one thing each; `draw_header` renders the first whose
/// left side leaves room for the whole right side (status chip, date, clock).
#[derive(Clone)]
struct HeaderRung {
    /// The wordmark: `" GAMEDAY "`, or `" GD "` on the last rungs.
    mark: &'static str,
    /// Whether tab chips wear their brackets (`[ NFL ]` vs `NFL`).
    bracketed: bool,
    /// The window of the tab list that renders; always holds the selected
    /// tab. Trailing tabs shed first, leading ones only as a last resort.
    shown: Range<usize>,
}

impl App {
    /// The header row. Its rule (spec R12): the right side — sort chip,
    /// status chip, date, clock — is never dropped and never clipped. A
    /// clock that vanishes on a 120-column terminal is worse than a tab bar
    /// that reads `NFL NBA` instead of `[ NFL ] [ NBA ]`, so the LEFT side is
    /// what gives, in this order: the chips' brackets, trailing league tabs
    /// (never past the selected one), and finally the wordmark `GAMEDAY` →
    /// `GD`. Spec §1: there is no `FILTER:` label anywhere on this ladder —
    /// v3.1's rung 0 is gone, and the bracketed chip row is the default.
    pub(super) fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let width = area.width as usize;
        let now = self.now();

        // --- The right side, measured first: everything else fits around it.
        // The connection chip is its own cell, padded on both sides so it can
        // never read as a suffix of the date ("OFFLINEMON SEP 1"). It is
        // never chopped mid-word — it degrades whole: padded label, bare
        // state word, then the word with no padding.
        let net = self.net.chip(Instant::now(), self.stale_after());
        let chip_forms: Vec<String> = match (net.label(), net.short_label()) {
            (Some(full), Some(bare)) => vec![format!("  {full}  "), format!(" {bare} "), bare],
            _ => Vec::new(),
        };
        // While the current tab is date-traveled the viewed date replaces the
        // live one, marked ‹ › so a past/future slate can't pass for today.
        let traveled = match self.tab {
            Tab::League(league) => self.viewed_date(league),
            Tab::Home => None,
        };
        let (date, date_style) = match traveled {
            Some(d) => (
                format!("‹ {} ›", date_label(d)),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ),
            None => (
                format!("{} {}", date_label(now.date()), now.year()),
                Style::default().fg(th.green).add_modifier(Modifier::BOLD),
            ),
        };
        let clock = crate::text::fmt_clock12(now);
        let date_len = date.chars().count() + 2;
        let clock_len = clock.chars().count() + 1;

        // The sort chip (spec §1: `s SORT: WATCH`) — Board view only. It
        // sits on the right, ahead of the NetStatus chip, and — like that
        // chip — degrades to nothing rather than ever clip the clock.
        let sort_text = (self.view == View::Board)
            .then(|| format!("s SORT: {} ", self.config.sort.label()));
        let sort_len = sort_text.as_ref().map_or(0, |s| s.chars().count());

        // --- The left side, as measurements rather than spans: the shed
        // ladder below picks one shape, and only then does it get rendered.
        let labels: Vec<(String, Tab)> = self
            .tab_list()
            .into_iter()
            // Spec §1: only an enabled league with a game today earns a
            // chip; the rest stay reachable via `:league`. The active tab is
            // exempt — switching to a quiet league must not erase its own
            // highlight.
            .filter(|tab| match tab {
                Tab::Home => true,
                Tab::League(league) => {
                    *tab == self.tab || self.boards.get(league).is_some_and(|v| !v.is_empty())
                }
            })
            .map(|tab| {
                let label = match tab {
                    Tab::Home => "ALL".to_string(),
                    Tab::League(league) => league.slug().to_uppercase(),
                };
                (label, tab)
            })
            .collect();
        let selected_idx = labels.iter().position(|(_, t)| *t == self.tab).unwrap_or(0);
        let alert_len = self
            .active_alert
            .as_ref()
            .map(|a| a.text.chars().count() + 2)
            .unwrap_or(0);
        // Rendered width of one chip, plus the space that follows it.
        let chip_cells = |label: &str, selected: bool, bracketed: bool| -> usize {
            let inner = label.chars().count();
            let framed = match (bracketed, selected) {
                (false, _) => inner,
                (true, true) => inner + 2,   // [NFL]
                (true, false) => inner + 4,  // [ NFL ]
            };
            framed + 1
        };
        // `shown` is a window into the tab list, not a prefix count: trailing
        // tabs shed first, and the leading ones only once there is nothing
        // else left to give. The selected tab is always inside it.
        let left_len = |mark: &str, bracketed: bool, shown: Range<usize>| -> usize {
            mark.chars().count()
                + labels[shown.clone()]
                    .iter()
                    .enumerate()
                    .map(|(i, (l, _))| chip_cells(l, shown.start + i == selected_idx, bracketed))
                    .sum::<usize>()
                + alert_len
        };
        // The chip is always followed by the date or the clock, so a form
        // with no trailing padding costs one extra column: the separator
        // that keeps "OFFLINE" from reading as "OFFLINEMON SEP 1".
        let chip_width =
            |form: &str| form.chars().count() + usize::from(!form.ends_with(' '));

        // The ladder, most generous first. Each rung gives up exactly one
        // thing, and the first rung whose left side plus the whole right side
        // fits the row is what renders.
        let n = labels.len();
        let rung = |mark, bracketed, shown| HeaderRung {
            mark,
            bracketed,
            shown,
        };
        let mut ladder = vec![rung(" GAMEDAY ", true, 0..n)];
        for end in (selected_idx + 1..=n).rev() {
            ladder.push(rung(" GAMEDAY ", false, 0..end));
        }
        for start in 0..=selected_idx {
            ladder.push(rung(" GD ", false, start..selected_idx + 1));
        }

        // Try the chip's forms longest-first at every rung: a padded chip
        // that fits is always better than a bare one.
        let best_chip = |left: usize, date: usize| -> Option<Option<String>> {
            if chip_forms.is_empty() {
                return (left + date + clock_len <= width).then_some(None);
            }
            chip_forms
                .iter()
                .find(|f| left + chip_width(f) + date + clock_len <= width)
                .map(|f| Some(f.clone()))
        };
        // The sort chip tries present, then absent, at every rung — it gives
        // way before the net chip degrades, but never before the clock does.
        let mut fit: Option<(HeaderRung, bool, Option<String>, bool)> = None;
        'ladder: for r in ladder.iter().cloned() {
            let left = left_len(r.mark, r.bracketed, r.shown.clone());
            for use_sort in [true, false] {
                if use_sort && sort_text.is_none() {
                    continue;
                }
                let sort_w = if use_sort { sort_len } else { 0 };
                if let Some(chip) = best_chip(left + sort_w, date_len) {
                    fit = Some((r, use_sort, chip, true));
                    break 'ladder;
                }
            }
        }
        // Under ~45 columns not even one tab plus the right side fits. The
        // clock is the last thing standing — it is the one thing the board
        // below never repeats — so the date goes first, then the chip (the
        // sort chip is long gone by this point).
        let (shape, use_sort, chip_text, show_date) = fit.unwrap_or_else(|| {
            let r = ladder.last().expect("ladder is non-empty").clone();
            let left = left_len(r.mark, r.bracketed, r.shown.clone());
            let chip = best_chip(left, 0).unwrap_or(None);
            (r, false, chip, false)
        });
        let HeaderRung {
            mark,
            bracketed,
            shown,
        } = shape;

        // --- Render the shape we chose.
        let mut spans = vec![Span::styled(
            mark,
            Style::default().fg(th.live).add_modifier(Modifier::BOLD),
        )];
        for (i, (label, tab)) in labels[shown.clone()].iter().enumerate() {
            let selected = shown.start + i == selected_idx;
            let text = match (bracketed, selected) {
                (false, _) => label.clone(),
                (true, true) => format!("[{label}]"),
                (true, false) => format!("[ {label} ]"),
            };
            let style = if selected {
                Style::default()
                    .fg(th.bg)
                    .bg(th.star)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.muted)
            };
            // Register the chip as a click zone at its rendered columns (the
            // header is all single-width chars, so chars == cells). Only
            // rendered tabs are clickable.
            let x: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            let w = text.chars().count();
            if x + w <= width {
                self.hit_zones.push((
                    Rect {
                        x: area.x + x as u16,
                        y: area.y,
                        width: w as u16,
                        height: 1,
                    },
                    keymap::Hit::TabChip(*tab),
                ));
            }
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" "));
        }
        // Favorite-score banner: earned red — the live role, spec's color
        // discipline — for its short lifetime, then advance_tick drops it.
        if let Some(alert) = &self.active_alert {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                alert.text.clone(),
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            ));
        }
        let chip_span = chip_text.map(|text| {
            let color = match net {
                NetChip::NoDataYet => th.muted,
                NetChip::Stale { .. } => th.star,
                // Offline is the one failure the board can have; it earns the
                // live role's red for as long as it lasts.
                NetChip::Offline { .. } => th.live,
                NetChip::Live => th.muted,
            };
            Span::styled(text, Style::default().fg(color).add_modifier(Modifier::BOLD))
        });
        let rendered_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let chip_pad = chip_span
            .as_ref()
            .is_some_and(|s| !s.content.ends_with(' '));
        let sort_render_len = if use_sort { sort_len } else { 0 };
        let right_len = sort_render_len
            + chip_span
                .as_ref()
                .map_or(0, |s| s.content.chars().count() + usize::from(chip_pad))
            + if show_date { date_len } else { 0 }
            + clock_len;
        let spacer = width.saturating_sub(rendered_left + right_len);
        spans.push(Span::raw(" ".repeat(spacer)));
        if use_sort {
            // `sort_text` is `Some` whenever `use_sort` is true (the search
            // above never sets it otherwise): key "s" bright, the rest muted,
            // like the footer's own key-cap discipline.
            let text = sort_text.as_deref().unwrap_or_default();
            let (key, rest) = text.split_at(1);
            spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(th.fg).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(rest.to_string(), Style::default().fg(th.muted)));
        }
        if let Some(s) = chip_span {
            spans.push(s);
            if chip_pad {
                spans.push(Span::raw(" "));
            }
        }
        if show_date {
            spans.push(Span::styled(date, date_style));
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            clock,
            Style::default().fg(th.clock()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    /// One SCORES lane, gated by `layout::plan(...).scores_lane` in
    /// `App::draw` (spec §1) — the Board never reaches this at all, since it
    /// draws its own inline off-screen lane (one lane, one owner).
    pub(super) fn draw_ticker(&self, frame: &mut Frame, area: Rect) {
        let d = self.derived();
        ticker::draw_lane(frame, area, &d.ticker_live, self.tick);
    }

    /// Context-aware footer: the TOP chords from the keymap table (the full
    /// set lives in the '?' overlay) plus position + freshness on the right.
    pub(super) fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        // An open prompt owns the whole footer row; a status line (command
        // error, pin result) owns it until the next keypress dismisses it.
        let prompt = match &self.mode {
            InputMode::Command { buf } => Some((':', buf)),
            InputMode::Filter { buf } => Some(('/', buf)),
            InputMode::Normal => None,
        };
        if let Some((sigil, buf)) = prompt {
            let mut line_spans = vec![
                Span::styled(
                    format!(" {sigil}"),
                    Style::default().fg(th.star).add_modifier(Modifier::BOLD),
                ),
                Span::styled(buf.clone(), Style::default().fg(th.bright)),
                Span::styled("▌", Style::default().fg(th.star)),
            ];
            // Tab-completion is a cycle, so the buffer alone never says what
            // else Tab would reach. List the stem's matches after the prompt
            // with the current one lit, so cycling is a choice, not a guess.
            if let (':', Some(state)) = (sigil, &self.completion) {
                let matches = crate::command::complete(&state.stem);
                if !matches.is_empty() {
                    line_spans.push(Span::styled("  ▸", Style::default().fg(th.muted)));
                    for (i, m) in matches.iter().enumerate() {
                        let style = if i == state.idx {
                            Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(th.muted)
                        };
                        line_spans.push(Span::styled(format!(" {m}"), style));
                        line_spans.push(Span::raw(" "));
                    }
                }
            }
            let line = Line::from(line_spans);
            frame.render_widget(
                Paragraph::new(line).style(Style::default().bg(th.bg)),
                area,
            );
            return;
        }
        if let Some(status) = &self.status_line {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {status}"),
                    Style::default().fg(th.star),
                )))
                .style(Style::default().bg(th.bg)),
                area,
            );
            return;
        }
        // Footer chords follow the view: the board advertises zoom + quit,
        // every other view advertises the way back (zoom its tab cycle, the
        // config editor its toggle/edit/cycle verbs, the feeds just the
        // shared chords — they have no tabs to cycle).
        let zoomed = self.view != View::Board;
        let ctx = match self.view {
            View::Board => keymap::FooterCtx::Board,
            View::ConfigView => keymap::FooterCtx::Config,
            View::Zoom { .. } => keymap::FooterCtx::Zoomed,
            View::PlaysFeed | View::Standings(_) | View::ThemePicker => keymap::FooterCtx::Feed,
        };
        // " /kc" steals footer columns, so it counts toward every shed budget
        // below regardless of which legend renders.
        let filter_width = self.filter.as_ref().map_or(0, |f| f.chars().count() + 2);
        let mut spans = Vec::new();
        // An active committed filter stays visible so a narrowed board is
        // never mistaken for a quiet one.
        if let Some(f) = &self.filter {
            spans.push(Span::styled(
                format!(" /{f}"),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ));
        }
        if ctx == keymap::FooterCtx::Board {
            // Spec §1: the Board footer is the fixed A′ legend — lowercase,
            // no brackets, no `NAV:` label. CMD earns no slot here (still
            // reachable via `:` and the help overlay); SORT/TV get real
            // KEYMAP rows in Task 10.
            let mut pairs: Vec<(&str, &str)> = keymap::BOARD_LEGEND.to_vec();
            let legend_width = |ps: &[(&str, &str)]| -> usize {
                filter_width
                    + 1
                    + ps.iter()
                        .map(|(k, l)| k.chars().count() + l.chars().count() + 3)
                        .sum::<usize>()
            };
            for drop in keymap::BOARD_LEGEND_DROP_ORDER {
                if legend_width(&pairs) < area.width as usize {
                    break;
                }
                pairs.retain(|(_, l)| l != drop);
            }
            for (key, label) in pairs {
                spans.push(Span::styled(format!(" {key}"), Style::default().fg(th.fg)));
                spans.push(Span::styled(format!(" {label} "), Style::default().fg(th.muted)));
            }
        } else {
            // Narrow terminals can't hold every chord: shed the low-value
            // ones in keymap's declared order so HELP and QUIT are never the
            // ones clipped.
            let mut chords = keymap::footer_chords(ctx);
            let chords_width = |cs: &[(&str, &str)]| -> usize {
                filter_width
                    + 5
                    + cs.iter()
                        .map(|(k, a)| 4 + k.chars().count() + a.chars().count())
                        .sum::<usize>()
            };
            for drop in keymap::FOOTER_DROP_ORDER {
                if chords_width(&chords) < area.width as usize {
                    break;
                }
                chords.retain(|(_, a)| a != drop);
            }
            spans.push(Span::styled(
                " NAV:",
                Style::default().fg(th.fg).add_modifier(Modifier::BOLD),
            ));
            for (key, action) in chords {
                spans.push(Span::styled(format!(" [{key}]"), Style::default().fg(th.fg)));
                spans.push(Span::styled(format!(" {action}"), Style::default().fg(th.muted)));
            }
        }

        // Right side, dropped piecewise if the row runs out of columns:
        // GAME 3/8 goes first, UPD stays.
        let mut right: Vec<String> = Vec::new();
        if zoomed {
            if let Some(g) = self.zoomed_game() {
                right.push(format!("FOCUS {}@{}", g.away.abbr, g.home.abbr));
            }
        } else {
            let sel_len = self.derived().selection.len();
            if sel_len > 1 {
                right.push(format!("GAME {}/{}", self.selected + 1, sel_len));
            }
        }
        // v3.2 §7: no pages — the board is one scrolling list, so GAME x/y is
        // the whole position report.
        // The UPD age freezes and dims the moment the data stops arriving —
        // `net` marks the frozen label with a trailing "·" so a stale number
        // can't pass for a live one.
        if let Some(upd) = self.net.upd_label(Instant::now(), self.stale_after()) {
            right.push(upd);
        }
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let width = area.width as usize;
        while !right.is_empty() && left_len + right.join("  ").chars().count() + 2 > width {
            right.remove(0);
        }
        if !right.is_empty() {
            let text_len = right.join("  ").chars().count();
            let spacer = width.saturating_sub(left_len + text_len + 1);
            spans.push(Span::raw(" ".repeat(spacer)));
            for (i, part) in right.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                // GAME/UPD is status, clock-shaped: it takes the clocks
                // discipline (cyan on broadcast, muted on studio), never raw
                // cyan — except a frozen UPD, which drops to dim.
                let color = if part.ends_with('·') { th.dim } else { th.clock() };
                spans.push(Span::styled(part.clone(), Style::default().fg(color)));
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
            area,
        );
    }

    /// '?': every chord, grouped, over a luminance-dimmed board. Generated
    /// from the same keymap table as the footer.
    pub(super) fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let th = theme::current();
        let buf = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                cell.fg = theme::dimmed(cell.fg);
                cell.bg = theme::dimmed(cell.bg);
            }
        }
        let mut lines: Vec<Line> = Vec::new();
        for group in keymap::Group::ALL {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                group.title(),
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            )));
            for (keys, label) in keymap::help_rows(group) {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {keys:<22}"), Style::default().fg(th.fg)),
                    Span::styled(label, Style::default().fg(th.muted)),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "ESC/?/Q CLOSES",
            Style::default().fg(th.dim),
        )));
        let w = 40u16.min(area.width.saturating_sub(4));
        let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let panel = Rect {
            x: area.x + (area.width - w) / 2,
            y: area.y + (area.height - h) / 2,
            width: w,
            height: h,
        };
        frame.render_widget(Clear, panel);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(th.star))
            .title(Span::styled(
                " KEYS ",
                Style::default().fg(th.star).add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(th.bg).fg(th.fg)),
            panel,
        );
    }
}
