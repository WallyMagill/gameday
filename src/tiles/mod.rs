pub mod logo;
pub mod packer;

use crate::domain::{Game, Meter, Status};
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density { Full, Standard, Compact }

pub fn render_tile(frame: &mut Frame, area: Rect, game: &Game, density: Density, selected: bool) {
    let border = if selected { theme::AMBER } else { theme::BORDER };
    let live = game.status == Status::Live;
    let title_right = if live { " LIVE " } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Line::from(vec![
            Span::styled(format!(" {} ", game.away.abbr), Style::default().fg(theme::rgb(game.away.color)).add_modifier(Modifier::BOLD)),
            Span::styled("@", Style::default().fg(theme::MUTED)),
            Span::styled(format!(" {} ", game.home.abbr), Style::default().fg(theme::rgb(game.home.color)).add_modifier(Modifier::BOLD)),
        ]))
        .title(Line::from(Span::styled(
            if live { title_right.to_string() } else { format!(" {} {} ", game.period, game.clock) },
            Style::default().fg(if live { theme::LIVE } else { theme::MUTED }).add_modifier(Modifier::BOLD),
        )).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let score = Line::from(vec![
        Span::styled(game.away.abbr.clone(), Style::default().fg(theme::rgb(game.away.color))),
        Span::raw(" "),
        Span::styled(game.away_score.to_string(), Style::default().fg(theme::rgb(game.away.color)).add_modifier(Modifier::BOLD)),
        Span::styled(" - ", Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
        Span::styled(game.home_score.to_string(), Style::default().fg(theme::rgb(game.home.color)).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(game.home.abbr.clone(), Style::default().fg(theme::rgb(game.home.color))),
        Span::raw("  "),
        Span::styled(format!("{} {}", game.period, game.clock), Style::default().fg(theme::MUTED)),
    ]);

    match density {
        Density::Compact => {
            frame.render_widget(Paragraph::new(score), inner);
        }
        Density::Standard | Density::Full => {
            let play_n = if density == Density::Full { 8 } else { 3 };
            let mut lines = vec![score];
            if let Some(sit) = &game.situation {
                let mut spans = vec![Span::styled(sit.down_distance.clone(), Style::default().fg(theme::AMBER))];
                if let Some(p) = &sit.possession {
                    spans.push(Span::styled(format!("  poss {p}"), Style::default().fg(theme::FG)));
                }
                if let Some(b) = &sit.ball_on {
                    spans.push(Span::styled(format!("  {b}"), Style::default().fg(theme::MUTED)));
                }
                lines.push(Line::from(spans));
            }
            if let Some(Meter::RedZone { yards_to_goal }) = game.meter {
                lines.push(Line::from(Span::styled(
                    format!("RZ {yards_to_goal}"),
                    Style::default().fg(theme::LIVE),
                )));
            }
            for p in game.last_plays.iter().take(play_n) {
                lines.push(Line::from(vec![
                    Span::styled("last  ", Style::default().fg(theme::MUTED)),
                    Span::styled(p.text.clone(), Style::default().fg(theme::FG)),
                ]));
            }
            if inner.width >= 24 && inner.height >= 6 && density != Density::Compact {
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(8), Constraint::Min(8)])
                    .split(inner);
                let away_slot = Rect {
                    x: cols[0].x,
                    y: cols[0].y,
                    width: cols[0].width.min(8),
                    height: cols[0].height.min(5),
                };
                logo::draw_logo(frame, away_slot, &game.away);
                if cols[0].height >= 10 {
                    let home_slot = Rect {
                        x: cols[0].x,
                        y: cols[0].y + 5,
                        width: cols[0].width.min(8),
                        height: 5,
                    };
                    logo::draw_logo(frame, home_slot, &game.home);
                }
                frame.render_widget(Paragraph::new(lines), cols[1]);
            } else {
                frame.render_widget(Paragraph::new(lines), inner);
            }
        }
    }
}
