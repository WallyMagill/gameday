//! The frame: `draw` derives the per-frame game lists once, `draw_frame`
//! lays out header, body, SCORES lane and footer around them.

use super::App;
use crate::theme;
use crate::ticker;
use crate::views::{self, View};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

impl App {
    /// One frame. The game lists are derived ONCE here and parked in
    /// `frame_cache`; every widget below reads them through `derived()`
    /// instead of re-walking the boards. The cache is dropped again on the
    /// way out — a list read outside a draw is a stale list, so `derived()`
    /// panics there rather than answering.
    pub fn draw(&mut self, frame: &mut Frame) {
        self.frame_cache = Some(self.derive());
        self.draw_frame(frame);
        self.frame_cache = None;
    }

    fn draw_frame(&mut self, frame: &mut Frame) {
        // Mouse zones are rebuilt from scratch every frame: whatever this
        // draw doesn't register is not clickable.
        self.hit_zones.clear();
        let th = theme::current();
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(th.bg).fg(th.fg)),
            area,
        );
        if area.width < 40 || area.height < 12 {
            // Walter's rule: a limit someone can hit must name the actual and
            // expected values — "need more columns" didn't say how many, or
            // whether it was rows that were short.
            frame.render_widget(
                Paragraph::new(format!("need 40×12, have {}×{}", area.width, area.height))
                    .style(Style::default().fg(th.muted).bg(th.bg)),
                area,
            );
            return;
        }
        // The Board has no separate ticker rows at all — it draws
        // its own one-row SCORES lane inline, inside the body, only when
        // something didn't fit (one lane, one owner; see `board::mod`'s
        // `draw_lane`). Every other view gets the same off-screen lane at
        // the bottom of the frame, gated by the SAME truncation the Board
        // would show at this size: `layout::plan(...).scores_lane`, run
        // against the current tab's counts.
        // TV is on the same footing as the Board here for the same reason:
        // it draws its own bottom strip of everything else that is live, and
        // a SCORES lane under that would be two lanes saying one thing.
        let ticker_h = if matches!(self.view, View::Board | View::ThemePicker | View::Tv) {
            0
        } else {
            let d = self.derived();
            let hero_in_band = d.my_games.iter().any(|g| Some(&g.id) == d.hero_id.as_ref());
            let band_rows = d.my_games.len() - usize::from(hero_in_band);
            let body_h = area.height.saturating_sub(2); // header + footer
            let plan = crate::board::layout::plan(
                area.width,
                body_h,
                d.in_play.len(),
                d.finals.len(),
                d.later.len(),
                band_rows,
            );
            if plan.scores_lane {
                ticker::LANE_HEIGHT
            } else {
                0
            }
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(ticker_h),
                Constraint::Length(1),
            ])
            .split(area);
        self.draw_header(frame, chunks[0]);
        // The cut. The takeover owns everything under the header —
        // the board is not drawn behind it at all — and the band is two rows
        // inserted above whatever the view was going to draw.
        let cut = self.cuts.active(self.tick).cloned();
        if let Some(cut) = cut.filter(|c| c.full) {
            if let Some(game) = self.game_by_id(&cut.game_id) {
                let below = Rect {
                    y: area.y + 1,
                    height: area.height - 1,
                    ..area
                };
                crate::board::cut::draw_takeover(frame, below, &game, &cut, self.tick);
                return;
            }
        }
        let mut body = chunks[1];
        // `!c.full` is explicit rather than implied by the early return above:
        // a full cut whose game left the board falls through to here, and a
        // takeover must never degrade into a band.
        let band = self
            .cuts
            .active(self.tick)
            .filter(|c| !c.full)
            .cloned()
            .and_then(|cut| self.game_by_id(&cut.game_id).map(|game| (cut, game)))
            .filter(|_| body.height > crate::board::cut::BAND_ROWS)
            .map(|(cut, game)| {
                (
                    Rect {
                        height: crate::board::cut::BAND_ROWS,
                        ..body
                    },
                    cut,
                    game,
                )
            });
        // The Board and TV RESERVE the band's rows up front
        // (`layout::TierPlan::band_rows`), so the band is painted over rows
        // they already set aside and nothing moves. Every other view is a
        // measured block or a list of its own with no reservation, so there
        // the band still costs the body two rows — the older behavior, and the
        // only place a fire still shifts anything.
        //
        // The one reserving case with no reservation to land on: a board with
        // nothing live (`band_rows == 0`) whose last game went final ON the
        // scoring play that fired the cut. The band then covers a section rule
        // for its three seconds instead of moving it — still no jump, which is
        // the property being bought, and not worth a row on every finals-only
        // board to avoid.
        let reserves_band = matches!(self.view, View::Board | View::ThemePicker | View::Tv);
        if let (Some((slot, ..)), false) = (&band, reserves_band) {
            body = Rect {
                y: slot.bottom(),
                height: body.height - slot.height,
                ..body
            };
        }
        let content_end = views::draw(self, frame, body);
        // After the view, not before: a reserved band is drawn ON the rows the
        // view just laid out (and left for it), so it has to land last.
        if let Some((slot, cut, game)) = band {
            crate::board::cut::draw_band(frame, slot, &game, &cut, self.tick);
        }
        if ticker_h > 0 {
            self.draw_ticker(frame, chunks[2]);
        }
        // A view that draws a measured block (the config editor)
        // keeps its key bar with the block — one row under the last content
        // row — instead of stranding it on the terminal floor. A SCORES lane
        // owns the bottom of the frame when it renders, so the footer stays
        // put underneath it rather than leapfrogging it.
        let footer = match content_end {
            Some(end) if ticker_h == 0 && end + 1 < chunks[3].y => Rect {
                y: end + 1,
                height: 1,
                ..chunks[3]
            },
            _ => chunks[3],
        };
        self.draw_footer(frame, footer);
        if self.help_open {
            self.draw_help(frame, area);
        }
    }
}
