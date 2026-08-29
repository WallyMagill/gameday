pub mod logo;
pub mod packer;

use crate::domain::{Game, Meter, Status, Team};
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density { Full, Standard, Compact }

/// Per-tile animation state, decided by the caller as a pure function of the
/// app's render tick — tiles themselves never look at a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileFx {
    /// Score cells render inverted (bg = live color) during the ~1s
    /// score-change flash.
    pub flash: bool,
    /// LIVE chip pulse phase: bright or the dimmed luminance step.
    pub live_bright: bool,
}

impl Default for TileFx {
    fn default() -> Self {
        Self { flash: false, live_bright: true }
    }
}

const LOGO_W: u16 = 10;
const SCORE_W: u16 = 7;
const METER_W: u16 = 9;
const IDENTITY_H: u16 = 6;

pub fn render_tile(
    frame: &mut Frame,
    area: Rect,
    game: &Game,
    density: Density,
    selected: bool,
    fx: TileFx,
) {
    let th = theme::current();
    if density == Density::Compact {
        render_compact(frame, area, game, fx);
        return;
    }
    let accent = th.league_accent(game.league);
    let border = if selected { th.star } else { th.border };

    let mut left_title = vec![
        Span::styled(
            format!("[{}]", game.league.slug().to_uppercase()),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    left_title.push(match game.status {
        Status::Live => Span::styled("LIVE", live_chip_style(fx)),
        Status::Final => Span::styled("FINAL", Style::default().fg(th.muted).add_modifier(Modifier::BOLD)),
        Status::Pre => Span::styled("UPCOMING", Style::default().fg(th.muted)),
    });

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Line::from(left_title))
        .title(
            Line::from(Span::styled(
                format!(" {} ", situation_summary(game)),
                Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 8 || inner.width < 30 {
        // Too small for the full grammar: score line only.
        frame.render_widget(
            Paragraph::new(score_line(game, fx.flash)).alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(IDENTITY_H), Constraint::Min(1)])
        .split(inner);
    render_identity(frame, rows[0], game, fx.flash);

    let lower = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(METER_W)])
        .split(rows[1]);
    render_lower_left(frame, lower[0], game);
    render_meter(frame, lower[1], game);
}

/// Situation string on the top border, right side.
fn situation_summary(game: &Game) -> String {
    match game.status {
        Status::Pre => {
            let mut s = game.start_time.clone().unwrap_or_default();
            if let Some(b) = &game.broadcast {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(b);
            }
            s
        }
        Status::Final => "FINAL".into(),
        Status::Live => {
            let mut s = format!("{} {}", game.period, game.clock).trim().to_string();
            if let Some(sit) = &game.situation {
                if !sit.down_distance.is_empty() {
                    s.push_str(" | ");
                    s.push_str(&sit.down_distance.to_uppercase());
                    if let Some(ball) = &sit.ball_on {
                        s.push(' ');
                        s.push_str(ball);
                    }
                }
            }
            s
        }
    }
}

/// LIVE chip: bold live color when bright, the same hue stepped down in
/// luminance on the dim half of the pulse.
fn live_chip_style(fx: TileFx) -> Style {
    let th = theme::current();
    let fg = if fx.live_bright {
        th.live
    } else {
        theme::dimmed(th.live)
    };
    Style::default().fg(fg).add_modifier(Modifier::BOLD)
}

/// A/B experiment: `GAMEDAY_BIG_SCORES=1` renders 3-row sextant digits
/// instead of the single-row score. Kept until Walter picks one by eye.
fn big_scores_enabled() -> bool {
    std::env::var("GAMEDAY_BIG_SCORES").is_ok_and(|v| v == "1")
}

/// Logo | city/NAME/record | 27 - 24 | city/NAME/record | logo
fn render_identity(frame: &mut Frame, area: Rect, game: &Game, flash: bool) {
    if big_scores_enabled() {
        render_identity_big(frame, area, game, flash);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(LOGO_W),
            Constraint::Min(4),
            Constraint::Length(SCORE_W),
            Constraint::Min(4),
            Constraint::Length(LOGO_W),
        ])
        .split(area);
    logo::draw_logo(frame, cols[0], &game.away);
    render_team_id(frame, cols[1], &game.away);
    let score_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1), Constraint::Min(0)])
        .split(cols[2]);
    frame.render_widget(
        Paragraph::new(score_line(game, flash)).alignment(Alignment::Center),
        score_rows[1],
    );
    render_team_id(frame, cols[3], &game.home);
    logo::draw_logo(frame, cols[4], &game.home);
}

/// Big-score variant: logos at the edges, 3-row sextant digits in the middle,
/// names + records on single rows beneath (no city line — the digits take it).
fn render_identity_big(frame: &mut Frame, area: Rect, game: &Game, flash: bool) {
    let th = theme::current();
    use tui_big_text::{BigText, PixelSize};
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(LOGO_W),
            Constraint::Min(4),
            Constraint::Length(LOGO_W),
        ])
        .split(area);
    logo::draw_logo(frame, cols[0], &game.away);
    logo::draw_logo(frame, cols[2], &game.home);
    let center = cols[1];
    let away_s = game.away_score.to_string();
    let home_s = game.home_score.to_string();
    let widths = [away_s.len() as u16 * 4, 4, home_s.len() as u16 * 4];
    let total: u16 = widths.iter().sum();
    let x0 = center.x + center.width.saturating_sub(total) / 2;
    let mut x = x0;
    for (text, w, color) in [
        (away_s.as_str(), widths[0], theme::rgb(game.away.color)),
        ("-", widths[1], th.muted),
        (home_s.as_str(), widths[2], theme::rgb(game.home.color)),
    ] {
        let slot = Rect { x, y: center.y, width: w.min(center.width), height: 3.min(center.height) };
        // Flash: swapped colors — dark digit strokes on a live-color field.
        let style = if flash && text != "-" {
            Style::default().fg(th.bg).bg(th.live).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        frame.render_widget(
            BigText::builder()
                .pixel_size(PixelSize::Sextant)
                .style(style)
                .lines(vec![Line::from(text.to_string())])
                .build(),
            slot,
        );
        x += w;
    }
    if center.height >= 5 {
        let names = Rect { x: center.x, y: center.y + 4, width: center.width, height: 1 };
        let label = |t: &Team| {
            let mut s = t.name.to_uppercase();
            if !t.record.is_empty() {
                s.push(' ');
                s.push_str(&t.record);
            }
            s
        };
        let half = (center.width as usize).saturating_sub(2) / 2;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(&label(&game.away), half),
                Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
            ))),
            names,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(&label(&game.home), half),
                Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Right),
            names,
        );
    }
}

/// Score digits; during the ~1s score-change flash the cells invert to the
/// theme's live color (one-shot, then they settle back to team colors).
fn score_line(game: &Game, flash: bool) -> Line<'static> {
    let th = theme::current();
    let digit = |n: u16, team_color: [u8; 3]| {
        let style = if flash {
            Style::default().fg(th.bg).bg(th.live).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::rgb(team_color)).add_modifier(Modifier::BOLD)
        };
        Span::styled(n.to_string(), style)
    };
    Line::from(vec![
        digit(game.away_score, game.away.color),
        Span::styled(" - ", Style::default().fg(th.muted)),
        digit(game.home_score, game.home.color),
    ])
}

fn render_team_id(frame: &mut Frame, area: Rect, team: &Team) {
    let th = theme::current();
    let w = area.width as usize;
    let city = if team.location.is_empty() { String::new() } else { team.location.to_uppercase() };
    let name = team.name.to_uppercase();
    let record = if team.record.is_empty() { "--".into() } else { team.record.clone() };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(truncate(&city, w), Style::default().fg(th.muted))),
        Line::from(Span::styled(
            truncate(&name, w),
            Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(record, Style::default().fg(th.muted))),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

/// Momentum row, dim rule, LAST PLAYS label, play lines.
fn render_lower_left(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);
    frame.render_widget(momentum_line(game).alignment(Alignment::Center), rows[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width.saturating_sub(2) as usize),
            Style::default().fg(th.dim),
        ))
        .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            " LAST PLAYS",
            Style::default().fg(th.league_accent(game.league)).add_modifier(Modifier::BOLD),
        )),
        rows[2],
    );
    let play_area = rows[3];
    let mut lines = Vec::new();
    for p in game.last_plays.iter().take(play_area.height as usize) {
        let team_color = if p.team.eq_ignore_ascii_case(&game.away.abbr) {
            theme::rgb(game.away.color)
        } else if p.team.eq_ignore_ascii_case(&game.home.abbr) {
            theme::rgb(game.home.color)
        } else {
            th.fg
        };
        let clock = format!(" [{}]", if p.clock.is_empty() { "-:--" } else { &p.clock });
        let abbr = format!(" {:<3} ", p.team);
        let used = clock.chars().count() + abbr.chars().count();
        let text = truncate(&p.text, (play_area.width as usize).saturating_sub(used + 1));
        lines.push(Line::from(vec![
            Span::styled(clock, Style::default().fg(th.muted)),
            Span::styled(abbr, Style::default().fg(team_color).add_modifier(Modifier::BOLD)),
            Span::styled(text, Style::default().fg(th.fg)),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            " no plays yet",
            Style::default().fg(th.dim),
        )));
    }
    frame.render_widget(Paragraph::new(lines), play_area);
}

/// ▶▶▶ MOMENTUM ◀◀◀ — the side that made the most recent (scoring) play is lit.
fn momentum_line(game: &Game) -> Paragraph<'static> {
    let th = theme::current();
    let mover = game
        .last_plays
        .iter()
        .find(|p| p.scoring)
        .or_else(|| game.last_plays.first())
        .map(|p| p.team.clone())
        .or_else(|| game.situation.as_ref().and_then(|s| s.possession.clone()))
        .unwrap_or_default();
    let away_hot = mover.eq_ignore_ascii_case(&game.away.abbr);
    let home_hot = mover.eq_ignore_ascii_case(&game.home.abbr);
    let side = |hot: bool, color: [u8; 3]| {
        if hot {
            Style::default().fg(theme::rgb(color)).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.dim)
        }
    };
    Paragraph::new(Line::from(vec![
        Span::styled("▶ ▶ ▶", side(away_hot, game.away.color)),
        Span::styled("  MOMENTUM  ", Style::default().fg(th.muted)),
        Span::styled("◀ ◀ ◀", side(home_hot, game.home.color)),
    ]))
}

fn render_meter(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let Some(meter) = &game.meter else { return };
    let accent = th.league_accent(game.league);
    let label_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    match meter {
        Meter::RedZone { yards_to_goal } => {
            lines.push(Line::from(Span::styled("RED ZONE", Style::default().fg(th.live).add_modifier(Modifier::BOLD))));
            let h = area.height.saturating_sub(1).max(1);
            // Gauge: 20 yards at the top, goal line at the bottom.
            let pos = ((20u16.saturating_sub(*yards_to_goal as u16)) * (h - 1).max(1) / 20).min(h - 1);
            for i in 0..h {
                let (sym, style) = if i == pos {
                    ("──█──", Style::default().fg(th.live).add_modifier(Modifier::BOLD))
                } else {
                    ("  ┃  ", Style::default().fg(th.dim))
                };
                let tag = match i {
                    i if i == h / 2 => " 10",
                    i if i == h - 1 => " G",
                    _ => "",
                };
                lines.push(Line::from(vec![
                    Span::styled(sym.to_string(), style),
                    Span::styled(tag.to_string(), Style::default().fg(th.muted)),
                ]));
            }
        }
        Meter::Lead { plus_minus } => {
            lines.push(Line::from(Span::styled("LEAD", label_style)));
            lines.push(Line::from(Span::styled("METER", label_style)));
            let h = area.height.saturating_sub(2).max(3);
            let span = 15i32;
            let pm = (*plus_minus as i32).clamp(-span, span);
            let pos = ((span - pm) * (h as i32 - 1) / (2 * span)) as u16;
            for i in 0..h {
                let tag = match i {
                    0 => "+15",
                    i if i == h / 2 => "  0",
                    i if i == h - 1 => "-15",
                    _ => "",
                };
                let (sym, style) = if i == pos {
                    ("─█─", Style::default().fg(if pm >= 0 { th.green } else { th.live }).add_modifier(Modifier::BOLD))
                } else {
                    (" ┃ ", Style::default().fg(th.dim))
                };
                lines.push(Line::from(vec![
                    Span::styled(sym.to_string(), style),
                    Span::styled(tag.to_string(), Style::default().fg(th.muted)),
                ]));
            }
        }
        Meter::Diamond { occupied } => {
            lines.push(Line::from(Span::styled("BASES", label_style)));
            lines.push(Line::from(""));
            let base = |on: bool| {
                if on {
                    Span::styled("◆", Style::default().fg(th.star).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("◇", Style::default().fg(th.dim))
                }
            };
            lines.push(Line::from(vec![Span::raw("   "), base(occupied[1])]));
            lines.push(Line::from(vec![
                Span::raw(" "),
                base(occupied[2]),
                Span::raw("   "),
                base(occupied[0]),
            ]));
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("▽", Style::default().fg(th.muted)),
            ]));
        }
        Meter::Penalty { team_abbr, seconds } => {
            lines.push(Line::from(Span::styled("PENALTY", label_style)));
            lines.push(Line::from(Span::styled("CLOCK", label_style)));
            lines.push(Line::from(""));
            if *seconds == 0 {
                lines.push(Line::from(Span::styled("--:--", Style::default().fg(th.dim))));
            } else {
                lines.push(Line::from(Span::styled(
                    team_abbr.clone(),
                    Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    format!("{}:{:02}", seconds / 60, seconds % 60),
                    Style::default().fg(th.live).add_modifier(Modifier::BOLD),
                )));
            }
        }
    }
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn render_compact(frame: &mut Frame, area: Rect, game: &Game, fx: TileFx) {
    let th = theme::current();
    let accent = th.league_accent(game.league);
    let score_style = if fx.flash {
        Style::default().fg(th.bg).bg(th.live).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
    };
    let mut spans = vec![
        Span::styled(
            format!("[{}] ", game.league.slug().to_uppercase()),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", game.away.abbr),
            Style::default().fg(theme::rgb(game.away.color)).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} - {}", game.away_score, game.home_score),
            score_style,
        ),
        Span::styled(
            format!(" {} ", game.home.abbr),
            Style::default().fg(theme::rgb(game.home.color)).add_modifier(Modifier::BOLD),
        ),
    ];
    match game.status {
        Status::Live => {
            spans.push(Span::styled(
                format!(" {} {} ", game.period, game.clock),
                Style::default().fg(th.muted),
            ));
            spans.push(Span::styled("LIVE", live_chip_style(fx)));
        }
        Status::Final => spans.push(Span::styled(" FINAL", Style::default().fg(th.muted))),
        Status::Pre => {
            if let Some(t) = &game.start_time {
                spans.push(Span::styled(format!(" {t}"), Style::default().fg(th.muted)));
            }
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        s.to_string()
    } else if width > 1 {
        let cut: String = s.chars().take(width - 1).collect();
        format!("{cut}…")
    } else {
        s.chars().take(width).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{League, Play, Situation};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn demo_game() -> Game {
        Game {
            id: "t1".into(),
            league: League::Nfl,
            away: Team {
                id: "12".into(),
                abbr: "KC".into(),
                name: "Chiefs".into(),
                location: "Kansas City".into(),
                record: "11-6".into(),
                color: [227, 24, 55],
                alt_color: [230, 230, 230],
                logo_key: "nfl/kc".into(),
            },
            home: Team {
                id: "27".into(),
                abbr: "TB".into(),
                name: "Buccaneers".into(),
                location: "Tampa Bay".into(),
                record: "11-6".into(),
                color: [213, 10, 10],
                alt_color: [230, 230, 230],
                logo_key: "nfl/tb".into(),
            },
            away_score: 27,
            home_score: 24,
            status: Status::Live,
            period: "Q4".into(),
            clock: "1:27".into(),
            situation: Some(Situation {
                down_distance: "1st & Goal".into(),
                possession: Some("KC".into()),
                ball_on: Some("TB 3".into()),
                ..Default::default()
            }),
            last_plays: vec![Play {
                clock: "1:27".into(),
                team: "KC".into(),
                text: "Mahomes pass to Kelce for 3 yards".into(),
                scoring: false,
            }],
            meter: Some(Meter::RedZone { yards_to_goal: 3 }),
            start_time: None,
            broadcast: Some("CBS".into()),
        }
    }

    fn render_buffer(game: &Game, density: Density, w: u16, h: u16, fx: TileFx) -> ratatui::buffer::Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_tile(f, f.area(), game, density, false, fx))
            .unwrap();
        term.backend().buffer().clone()
    }

    fn render_to_text(game: &Game, density: Density, w: u16, h: u16) -> String {
        let buf = render_buffer(game, density, w, h, TileFx::default());
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn standard_tile_has_redzone_grammar() {
        let text = render_to_text(&demo_game(), Density::Standard, 49, 15);
        assert!(text.contains("[NFL]"), "league chip missing:\n{text}");
        assert!(text.contains("LIVE"), "live badge missing");
        assert!(text.contains("27 - 24"), "score missing:\n{text}");
        assert!(text.contains("MOMENTUM"), "momentum missing:\n{text}");
        assert!(text.contains("LAST PLAYS"), "plays label missing:\n{text}");
        assert!(text.contains("RED ZONE"), "meter missing:\n{text}");
        assert!(text.contains("CHIEFS"), "team name missing:\n{text}");
        assert!(text.contains("1ST & GOAL TB 3"), "situation missing:\n{text}");
    }

    #[test]
    fn compact_tile_is_one_line() {
        let text = render_to_text(&demo_game(), Density::Compact, 49, 3);
        assert!(text.contains("KC 27 - 24 TB"), "compact score missing:\n{text}");
        assert!(text.contains("LIVE"));
    }

    #[test]
    fn flash_inverts_score_cells_and_settling_restores_them() {
        let th = theme::current();
        let count_live_bg = |fx: TileFx| {
            let buf = render_buffer(&demo_game(), Density::Standard, 49, 15, fx);
            let mut n = 0;
            for y in 0..15 {
                for x in 0..49 {
                    if buf[(x, y)].bg == th.live {
                        n += 1;
                    }
                }
            }
            n
        };
        let flashed = count_live_bg(TileFx { flash: true, live_bright: true });
        assert!(flashed >= 4, "score digits + dash should sit on the live bg, got {flashed}");
        let settled = count_live_bg(TileFx::default());
        assert_eq!(settled, 0, "no live bg once the flash settles");
    }

    #[test]
    fn live_chip_pulses_between_bright_and_dimmed() {
        let th = theme::current();
        let chip_fg = |bright: bool| {
            let buf = render_buffer(
                &demo_game(),
                Density::Standard,
                49,
                15,
                TileFx { flash: false, live_bright: bright },
            );
            // Find "LIVE" on the top border and return the L cell's fg.
            for x in 0..46u16 {
                let word: String = (0..4).map(|i| buf[(x + i, 0)].symbol().to_string()).collect();
                if word == "LIVE" {
                    return buf[(x, 0)].fg;
                }
            }
            panic!("LIVE chip not found");
        };
        assert_eq!(chip_fg(true), th.live);
        assert_eq!(chip_fg(false), theme::dimmed(th.live));
        assert_ne!(th.live, theme::dimmed(th.live), "dim step must be visible");
    }

    #[test]
    fn missing_logo_falls_back_to_abbr() {
        let mut game = demo_game();
        game.away.logo_key = "nfl/zzz".into();
        game.away.abbr = "ZZZ".into();
        let text = render_to_text(&game, Density::Standard, 49, 15);
        assert!(text.contains("⟨ZZZ⟩"), "abbr fallback missing:\n{text}");
    }
}
