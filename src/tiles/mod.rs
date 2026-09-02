pub mod logo;
pub mod packer;

use crate::domain::{Game, League, Meter, Status, Team};
use crate::text::truncate;
use crate::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density { Full, Standard, Compact }

/// How score digits render inside a tile: config key `score_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScoreStyle {
    /// 3-row sextant digits via tui_big_text — the jumbotron look (default).
    #[default]
    Big,
    /// Single-row "27 - 24" between the team identity columns.
    Compact,
}

/// Per-tile animation state, decided by the caller as a pure function of the
/// app's render tick — tiles themselves never look at a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileFx {
    /// Score cells render inverted (bg = live color) during the ~1s
    /// score-change flash.
    pub flash: bool,
    /// LIVE chip pulse phase: bright or the dimmed luminance step.
    pub live_bright: bool,
    /// This game is pinned: the title wears a ⚑.
    pub pinned: bool,
    /// Either team is a favorite of this league: the title wears a ★.
    pub favorite: bool,
    /// The frame's clock, in the user's offset. Start times render relative
    /// to this and nothing in here reads a wall clock, so a dump of a given
    /// tick is the same pixels every run.
    pub now: OffsetDateTime,
}

impl Default for TileFx {
    fn default() -> Self {
        Self {
            flash: false,
            live_bright: true,
            pinned: false,
            favorite: false,
            now: OffsetDateTime::UNIX_EPOCH,
        }
    }
}

const LOGO_W: u16 = 10;
const IDENTITY_H: u16 = 6;
/// Identity rows in the focus view when the full-size (8-row) digits fit:
/// 8 digit rows + gap + names row.
const IDENTITY_FULL_H: u16 = 10;
/// Narrowest track the RED ZONE / LEAD gauges draw: at 8 cells the red-zone
/// marker moves every ~2.5 yards and a ±2 lead visibly leaves center. A
/// by-eye pick, not a measurement — the row's value tag shortens before the
/// track is allowed to drop below it.
const METER_MIN_BAR: usize = 8;
/// Longest RED ZONE / LEAD track. The 2x2 board draws ~20 cells; at the zoom
/// view's 100+ a 20-yard gauge stops reading as a gauge (and sat on top of
/// the full-width drive bar as a near-duplicate). By eye.
const METER_MAX_BAR: usize = 40;
/// Longest penalty countdown bar: past 20 cells (6 s per cell for a minor)
/// the drain reads as a progress bar rather than a clock. By eye.
const PENALTY_BAR_MAX: usize = 20;
/// The minor penalty the countdown bar is scaled to (2:00 by rule; a major
/// shows as a full bar until it is inside its last two minutes).
const PENALTY_MINOR_SECS: u16 = 120;

pub fn render_tile(
    frame: &mut Frame,
    area: Rect,
    game: &Game,
    density: Density,
    selected: bool,
    fx: TileFx,
    score_style: ScoreStyle,
) {
    let th = theme::current();
    if density == Density::Compact {
        render_compact(frame, area, game, fx);
        return;
    }
    let border = if selected { th.star } else { th.border };

    let mut left_title = vec![
        Span::styled(
            format!("[{}]", game.league.slug().to_uppercase()),
            Style::default().fg(th.chip(game.league)).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    // Pin and favorite ride the title after the league chip: the two reasons
    // this tile is on the board, before the status word.
    if fx.pinned {
        left_title.push(Span::styled("⚑ ", Style::default().fg(th.star)));
    }
    if fx.favorite {
        left_title.push(Span::styled("★ ", Style::default().fg(th.star)));
    }
    left_title.push(match game.status {
        Status::Live => Span::styled("LIVE", live_chip_style(fx)),
        Status::Final => Span::styled("FINAL", Style::default().fg(th.muted).add_modifier(Modifier::BOLD)),
        Status::Pre => Span::styled("UPCOMING", Style::default().fg(th.muted)),
    });

    let mut right_title = vec![Span::styled(
        format!(" {} ", situation_summary(game, fx.now)),
        Style::default().fg(th.bright).add_modifier(Modifier::BOLD),
    )];
    // Basketball shot-clock chip: boxed amber badge, distinct from the game
    // clock. Renders only when the value is present (demo supplies it; the
    // real feed doesn't carry one — see provider::map).
    if let Some(sc) = shot_clock_of(game) {
        right_title.push(Span::styled(
            format!(" {sc} "),
            Style::default().fg(th.bg).bg(th.star).add_modifier(Modifier::BOLD),
        ));
        right_title.push(Span::raw(" "));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Line::from(left_title))
        .title(Line::from(right_title).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 4 || inner.width < 30 {
        // Too small for any grammar: score line only.
        frame.render_widget(
            Paragraph::new(score_line(game, fx.flash)).alignment(Alignment::Center),
            inner,
        );
        return;
    }
    if inner.height < 8 {
        // Short tile (80x24-class terminals): digits + names + momentum +
        // plays, packed row by row — never a header over a void.
        render_short(frame, inner, game, fx.flash, score_style);
        return;
    }

    // Focus view with room: the digits double in size (8-row LED glyphs).
    let full_digits = density == Density::Full
        && score_style == ScoreStyle::Big
        && inner.height >= IDENTITY_FULL_H + 8;
    let id_h = if full_digits { IDENTITY_FULL_H } else { IDENTITY_H };
    // Meter B: one gauge row under the identity block, before momentum; no
    // meter (soccer, no data) and the plays feed gets the line instead.
    let meter = meter_line(game, inner.width as usize);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(id_h),
            Constraint::Length(u16::from(meter.is_some())),
            Constraint::Min(1),
        ])
        .split(inner);
    match score_style {
        ScoreStyle::Big => render_identity_big(frame, rows[0], game, fx.flash, full_digits),
        ScoreStyle::Compact => render_identity(frame, rows[0], game, fx.flash),
    }
    if let Some(line) = meter {
        frame.render_widget(Paragraph::new(line), rows[1]);
    }
    if density == Density::Full {
        render_focus_body(frame, rows[2], game);
    } else {
        render_lower_left(frame, rows[2], game);
    }
}

/// Situation string on the top border, right side.
fn situation_summary(game: &Game, now: OffsetDateTime) -> String {
    match game.status {
        Status::Pre => {
            let mut s = game
                .start
                .map(|t| crate::text::fmt_start(t, now))
                .unwrap_or_default();
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

/// Live basketball shot clock, when the situation actually carries one.
fn shot_clock_of(game: &Game) -> Option<u8> {
    if game.status != Status::Live {
        return None;
    }
    match game.league {
        League::Nba | League::Wnba | League::Cbb => {
            game.situation.as_ref().and_then(|s| s.shot_clock)
        }
        _ => None,
    }
}

/// Compact score style: logo | city/NAME/record | 27 - 24 | … | logo.
/// The score column is sized to the actual score text plus one guaranteed
/// spacer cell per side, so a long name can never abut the digits.
fn render_identity(frame: &mut Frame, area: Rect, game: &Game, flash: bool) {
    let score_w = format!("{} - {}", game.away_score, game.home_score)
        .chars()
        .count() as u16;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(LOGO_W),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(score_w),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(LOGO_W),
        ])
        .split(area);
    logo::draw_logo(frame, cols[0], &game.away);
    // The city line is a pair decision: both sides or neither, so one card
    // never reads "CHIEFS / 11-6" beside "TAMPA BAY / TB / 11-6".
    let city_fits = |t: &Team, col: Rect| t.location.chars().count() <= col.width as usize;
    let with_city = city_fits(&game.away, cols[1]) && city_fits(&game.home, cols[5]);
    render_team_id(frame, cols[1], &game.away, with_city);
    let score_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1), Constraint::Min(0)])
        .split(cols[3]);
    frame.render_widget(
        Paragraph::new(score_line(game, flash)).alignment(Alignment::Center),
        score_rows[1],
    );
    render_team_id(frame, cols[5], &game.home, with_city);
    logo::draw_logo(frame, cols[6], &game.home);
}

/// Big digits for `game`'s score centered in `center`: sextant (4x3 cells per
/// glyph) or, when `full`, whole-cell LED glyphs (8x8). Returns false without
/// drawing when the digits don't fit `center` — every slot is also clamped to
/// the rect so a wider-than-expected glyph can never index past the buffer
/// (3-digit scores in narrow tiles panicked here before).
fn render_digits(frame: &mut Frame, center: Rect, game: &Game, flash: bool, full: bool) -> bool {
    let th = theme::current();
    use tui_big_text::{BigText, PixelSize};
    let (gw, gh, px) = if full {
        (8u16, 8u16, PixelSize::Full)
    } else {
        (4u16, 3u16, PixelSize::Sextant)
    };
    let away_s = game.away_score.to_string();
    let home_s = game.home_score.to_string();
    let widths = [away_s.len() as u16 * gw, gw, home_s.len() as u16 * gw];
    let total: u16 = widths.iter().sum();
    if total > center.width || gh > center.height {
        return false;
    }
    let x0 = center.x + (center.width - total) / 2;
    let mut x = x0;
    for (text, w, color) in [
        (away_s.as_str(), widths[0], theme::rgb(game.away.color)),
        ("-", widths[1], th.muted),
        (home_s.as_str(), widths[2], theme::rgb(game.home.color)),
    ] {
        if x >= center.right() {
            break;
        }
        let slot = Rect {
            x,
            y: center.y,
            width: w.min(center.right() - x),
            height: gh.min(center.height),
        };
        // Flash: swapped colors — dark digit strokes on a live-color field.
        let style = if flash && text != "-" {
            Style::default().fg(th.bg).bg(th.live).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };
        frame.render_widget(
            BigText::builder()
                .pixel_size(px)
                .style(style)
                .lines(vec![Line::from(text.to_string())])
                .build(),
            slot,
        );
        x += w;
    }
    true
}

/// One-row team labels under the digits: away left, home right, in `area`.
/// A record is worth more than the long name that crowds it out, so a side
/// whose "MARINERS 64-73" doesn't fit its half falls back to "SEA 64-73"
/// before the record is dropped. Whether records show at all is still a pair
/// decision — one side wearing a record while the other doesn't reads as a
/// missing one (board-*.png showed "CHIEFS 11-6" beside a bare "BUCCANEERS")
/// — but each side picks its own form.
///
/// The side with the ball wears a `▸`/`◂` pointing at the field, inside the
/// same half-width budget as its label.
fn render_name_row(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let half = (area.width as usize).saturating_sub(2) / 2;
    let possession = game
        .situation
        .as_ref()
        .and_then(|s| s.possession.as_deref())
        .filter(|p| !p.is_empty());
    let has_ball = |t: &Team| possession.is_some_and(|p| p.eq_ignore_ascii_case(&t.abbr));
    // "▸ " / " ◂" spend two of the side's cells.
    let budget = |t: &Team| half.saturating_sub(if has_ball(t) { 2 } else { 0 });
    // The longest form of "<label> <record>" that fits this side, if any.
    let with_record = |t: &Team| {
        if t.record.is_empty() {
            return None;
        }
        let fits = |s: &str| s.chars().count() + 1 + t.record.chars().count() <= budget(t);
        let name = t.name.to_uppercase();
        if fits(&name) {
            Some(format!("{name} {}", t.record))
        } else if fits(&t.abbr) {
            Some(format!("{} {}", t.abbr, t.record))
        } else {
            None
        }
    };
    // Both sides or neither: zip drops a lone record.
    let (away_form, home_form) = with_record(&game.away).zip(with_record(&game.home)).unzip();
    let label = |t: &Team, form: Option<String>, away: bool| {
        let body = form.unwrap_or_else(|| truncate(&t.name.to_uppercase(), budget(t)));
        match (has_ball(t), away) {
            (false, _) => body,
            (true, true) => format!("▸ {body}"),
            (true, false) => format!("{body} ◂"),
        }
    };
    let away_label = label(&game.away, away_form, true);
    let home_label = label(&game.home, home_form, false);
    let style = Style::default().fg(th.bright).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(away_label, style))),
        area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(home_label, style)))
            .alignment(Alignment::Right),
        area,
    );
}

/// Big score style (default): 3-row sextant digits in the middle (8-row LED
/// digits in the focus view), names + records on single rows beneath. Logos
/// flank the digits only when both fit beside them — on a narrow tile the
/// digits keep the whole width instead of falling back to a text score.
fn render_identity_big(frame: &mut Frame, area: Rect, game: &Game, flash: bool, full: bool) {
    let gw = if full { 8u16 } else { 4u16 };
    let digits_w =
        (game.away_score.to_string().len() + 1 + game.home_score.to_string().len()) as u16 * gw;
    let center = if area.width >= digits_w + 2 * LOGO_W + 2 {
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
        cols[1]
    } else {
        area
    };
    let mut gh = if full { 8 } else { 3 };
    let mut drawn = render_digits(frame, center, game, flash, full);
    if !drawn && full {
        // Focus view too tight for LED digits: sextant digits still land.
        gh = 3;
        drawn = render_digits(frame, center, game, flash, false);
    }
    if !drawn {
        // Nothing big fits: single-row score, never a blank identity.
        let row = Rect { x: center.x, y: center.y + 1, width: center.width, height: 1 };
        frame.render_widget(
            Paragraph::new(score_line(game, flash)).alignment(Alignment::Center),
            row,
        );
        gh = 2; // score row sits on row 1; names follow after the same gap
    }
    let name_y = center.y + gh + 1;
    if name_y < center.bottom() {
        let names = Rect { x: center.x, y: name_y, width: center.width, height: 1 };
        render_name_row(frame, names, game);
    }
}

/// Short tile (inner height 4..8): score, names, momentum, then plays for
/// whatever rows remain. The 80x24 four-up board lands here — it must read
/// as a scoreboard, not a header floating over empty rows.
fn render_short(frame: &mut Frame, inner: Rect, game: &Game, flash: bool, style: ScoreStyle) {
    let mut y = inner.y;
    let row = |y: u16| Rect { x: inner.x, y, width: inner.width, height: 1 };
    let digits = Rect { x: inner.x, y, width: inner.width, height: 3 };
    if style == ScoreStyle::Big && render_digits(frame, digits, game, flash, false) {
        y += 3;
    } else {
        frame.render_widget(
            Paragraph::new(score_line(game, flash)).alignment(Alignment::Center),
            row(y),
        );
        y += 1;
    }
    if y < inner.bottom() {
        render_name_row(frame, row(y), game);
        y += 1;
    }
    if y < inner.bottom() {
        if let Some(line) = meter_line(game, inner.width as usize) {
            frame.render_widget(Paragraph::new(line), row(y));
            y += 1;
        }
    }
    if y < inner.bottom() {
        frame.render_widget(momentum_line(game).alignment(Alignment::Center), row(y));
        y += 1;
    }
    if y < inner.bottom() {
        let plays = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: inner.bottom() - y,
        };
        render_play_lines(frame, plays, game);
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

/// Compact identity column: city / NAME / record. Identity strings are never
/// ellipsized ("KANSAS C…", "BUCCANEE…" read as typos): the city line is
/// shown only when the caller says both sides' cities fit (`with_city`), and
/// a name that doesn't fit falls back to the abbr, which always does.
fn render_team_id(frame: &mut Frame, area: Rect, team: &Team, with_city: bool) {
    let th = theme::current();
    let w = area.width as usize;
    let fits = |s: &str| s.chars().count() <= w;
    let city = if with_city { team.location.to_uppercase() } else { String::new() };
    let name = team.name.to_uppercase();
    let name = if fits(&name) { name } else { team.abbr.to_uppercase() };
    let record = if team.record.is_empty() { "--".into() } else { team.record.clone() };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(city, Style::default().fg(th.muted))),
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
            Style::default()
                .fg(th.section_label(th.league_accent(game.league)))
                .add_modifier(Modifier::BOLD),
        )),
        rows[2],
    );
    render_play_lines(frame, rows[3], game);
}

/// The bracket stamp on a play row: the game clock when the sport has one,
/// otherwise the play's period (baseball `B9`). `-:--` only when the feed
/// gave us neither.
fn play_stamp(p: &crate::domain::Play) -> &str {
    if !p.clock.is_empty() {
        &p.clock
    } else if !p.period.is_empty() {
        &p.period
    } else {
        "-:--"
    }
}

/// `[clock] ABB text` row for one play, truncated to `width`.
fn play_line(game: &Game, p: &crate::domain::Play, width: usize) -> Line<'static> {
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

fn render_play_lines(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let mut lines: Vec<Line> = game
        .last_plays
        .iter()
        .take(area.height as usize)
        .map(|p| play_line(game, p, area.width as usize))
        .collect();
    if lines.is_empty() {
        // A pre-game tile has no plays; its line is the betting line (dim —
        // odds are context, never chrome-loud).
        let empty = match (&game.status, &game.odds) {
            (Status::Pre, Some(odds)) => format!(" {odds}"),
            _ => " no plays yet".to_string(),
        };
        lines.push(Line::from(Span::styled(empty, Style::default().fg(th.dim))));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// Football field bar for the focus view (the spotify-progress steal): the
/// drive as a 100-yard bar with the ball on it, filled in the possessing
/// team's color toward the goal on the right. None when the situation
/// doesn't carry a parseable spot.
fn field_line(game: &Game, width: usize) -> Option<Line<'static>> {
    if !matches!(game.league, League::Nfl | League::Cfb) || game.status != Status::Live {
        return None;
    }
    let th = theme::current();
    let sit = game.situation.as_ref()?;
    let poss = sit.possession.as_deref()?;
    let (territory, yards) = sit.ball_on.as_deref()?.rsplit_once(' ')?;
    let yards: u16 = yards.parse().ok()?;
    if yards > 50 {
        return None;
    }
    // Yards left to the opponent's goal line.
    let to_goal = if territory.eq_ignore_ascii_case(poss) { 100 - yards } else { yards };
    let color = if poss.eq_ignore_ascii_case(&game.away.abbr) {
        theme::rgb(game.away.color)
    } else {
        theme::rgb(game.home.color)
    };
    let label = format!(" {poss} ");
    let bar_w = width.checked_sub(label.chars().count() + 4)?.max(10);
    let filled = (bar_w as u16 * (100 - to_goal) / 100).min(bar_w as u16 - 1) as usize;
    Some(Line::from(vec![
        Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled("━".repeat(filled), Style::default().fg(color)),
        Span::styled("●", Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(
            "─".repeat(bar_w.saturating_sub(filled + 1)),
            Style::default().fg(th.dim),
        ),
        Span::styled(" G ", Style::default().fg(th.muted)),
    ]))
}

/// Focus-view body (golazo steal: score + scoring ticker + drive): momentum,
/// the drive/field bar, the last-plays feed, then a SCORING timeline — the
/// drill-in must never show less than the tile it came from.
fn render_focus_body(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let accent = th.section_label(th.league_accent(game.league));
    let n_plays = game.last_plays.len().min(8) as u16;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                // momentum
            Constraint::Length(1),                // field bar / rule
            Constraint::Length(1),                // LAST PLAYS label
            Constraint::Length(n_plays.max(1)),   // plays feed
            Constraint::Length(1),                // rule
            Constraint::Min(0),                   // scoring timeline
        ])
        .split(area);
    frame.render_widget(momentum_line(game).alignment(Alignment::Center), rows[0]);
    // Inside the red zone the inline RED ZONE gauge already plots the ball;
    // a second dot on a 100-yard scale directly under it read as two
    // different spots, so the field bar yields to a plain rule there.
    let in_red_zone = matches!(game.meter, Some(Meter::RedZone { .. }));
    let divider = (!in_red_zone)
        .then(|| field_line(game, area.width as usize))
        .flatten()
        .unwrap_or_else(|| {
            Line::from(Span::styled(
                "─".repeat(area.width.saturating_sub(2) as usize),
                Style::default().fg(th.dim),
            ))
        });
    frame.render_widget(Paragraph::new(divider).alignment(Alignment::Center), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            " LAST PLAYS",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        rows[2],
    );
    render_play_lines(frame, rows[3], game);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width.saturating_sub(2) as usize),
            Style::default().fg(th.dim),
        ))
        .alignment(Alignment::Center),
        rows[4],
    );
    let scoring_area = rows[5];
    if scoring_area.height == 0 {
        return;
    }
    let mut lines = vec![Line::from(Span::styled(
        " SCORING",
        Style::default().fg(th.live).add_modifier(Modifier::BOLD),
    ))];
    let word = theme::scoring_word(game.league);
    for p in game.scoring_plays.iter().rev() {
        let team_color = if p.team.eq_ignore_ascii_case(&game.away.abbr) {
            th.team_text(game.away.color)
        } else {
            th.team_text(game.home.color)
        };
        let head = format!(" [{}] {:<3} ", play_stamp(p), p.team);
        let used = head.chars().count() + word.chars().count() + 1;
        lines.push(Line::from(vec![
            Span::styled(head, Style::default().fg(team_color).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{word} "),
                Style::default().fg(th.live).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate(&p.text, (scoring_area.width as usize).saturating_sub(used + 1)),
                Style::default().fg(th.fg),
            ),
        ]));
    }
    if lines.len() == 1 {
        lines.push(Line::from(Span::styled(
            " no scoring yet",
            Style::default().fg(th.dim),
        )));
    }
    lines.truncate(scoring_area.height as usize);
    frame.render_widget(Paragraph::new(lines), scoring_area);
}

/// ▶▶▶ MOMENTUM ◀◀◀ — BOTH sides tick in their team color (per the reference
/// board): the hot side bright and bold, the cold side the same hue stepped
/// down through the theme's dim luminance.
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
            Style::default().fg(theme::dimmed(theme::rgb(color)))
        }
    };
    Paragraph::new(Line::from(vec![
        Span::styled("▶ ▶ ▶", side(away_hot, game.away.color)),
        Span::styled("  MOMENTUM  ", Style::default().fg(th.muted)),
        Span::styled("◀ ◀ ◀", side(home_hot, game.home.color)),
    ]))
}

/// Meter B: the one-row inline gauge under the identity block. `None` when
/// the game carries no meter (soccer, pre/final, no data) — the caller gives
/// the row to the plays feed. Every row is `LABEL  track  VALUE`, left
/// aligned with the ` LAST PLAYS` caption; labels take the section-label
/// discipline, RED ZONE stays the earned live red, and the value tag
/// shortens rather than let the track fall under [`METER_MIN_BAR`] or the
/// row clip at the border.
fn meter_line(game: &Game, width: usize) -> Option<Line<'static>> {
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

/// Compact tile: `[NFL] KC 27 - 24 TB  Q4 1:27 LIVE`. The abbrs here ARE the
/// identity block (there is no name row or logo), so they keep raw team
/// color on every theme like the score digits do — the identity floor, not
/// the `play_abbrs` grant. Same rule for the zoom stats headers.
fn render_compact(frame: &mut Frame, area: Rect, game: &Game, fx: TileFx) {
    let th = theme::current();
    let accent = th.chip(game.league);
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
            if let Some(t) = game.start {
                let s = crate::text::fmt_start(t, fx.now);
                spans.push(Span::styled(format!(" {s}"), Style::default().fg(th.muted)));
            }
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
            }],
            meter: Some(Meter::RedZone { yards_to_goal: 3 }),
            broadcast: Some("CBS".into()),
            ..Game::default()
        }
    }

    fn render_buffer(game: &Game, density: Density, w: u16, h: u16, fx: TileFx, style: ScoreStyle) -> ratatui::buffer::Buffer {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_tile(f, f.area(), game, density, false, fx, style))
            .unwrap();
        term.backend().buffer().clone()
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer, w: u16, h: u16) -> String {
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn render_to_text(game: &Game, density: Density, w: u16, h: u16) -> String {
        let buf = render_buffer(game, density, w, h, TileFx::default(), ScoreStyle::Compact);
        buffer_text(&buf, w, h)
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
            let buf = render_buffer(&demo_game(), Density::Standard, 49, 15, fx, ScoreStyle::Compact);
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
        let flashed = count_live_bg(TileFx { flash: true, ..Default::default() });
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
                TileFx { live_bright: bright, ..Default::default() },
                ScoreStyle::Compact,
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
    fn score_style_selects_big_or_compact() {
        let g = demo_game();
        let big = buffer_text(
            &render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big),
            49,
            15,
        );
        let compact = buffer_text(
            &render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact),
            49,
            15,
        );
        assert!(compact.contains("27 - 24"), "compact = single-row score:\n{compact}");
        assert!(!big.contains("27 - 24"), "big renders sextant digits, not a text row:\n{big}");
        // 49 wide the identity row is the abbr form (see
        // name_row_records_are_a_pair_decision_and_never_truncate).
        assert!(big.contains("KC 11-6"), "big shows the identity row under the digits:\n{big}");
    }

    fn nba_game(shot_clock: Option<u8>) -> Game {
        let mut g = demo_game();
        g.league = League::Nba;
        g.situation = Some(Situation { shot_clock, ..Default::default() });
        g.meter = Some(Meter::Lead { plus_minus: 3 });
        g
    }

    #[test]
    fn shot_clock_chip_renders_only_when_the_value_is_present() {
        let th = theme::current();
        // Chip cells sit on the star (amber) background on the top border row.
        let chip_cells = |g: &Game| {
            let buf = render_buffer(g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
            (0..49u16).filter(|&x| buf[(x, 0)].bg == th.star).count()
        };
        assert!(chip_cells(&nba_game(Some(24))) >= 4, "boxed '24' badge missing");
        assert_eq!(chip_cells(&nba_game(None)), 0, "no value => no chip, never faked");
        // Non-basketball games never grow a chip even if data carried a value.
        let mut nfl = demo_game();
        if let Some(sit) = &mut nfl.situation {
            sit.shot_clock = Some(24);
        }
        assert_eq!(chip_cells(&nfl), 0, "shot clock is basketball-only");
    }

    #[test]
    fn momentum_ticks_both_sides_hot_bright_cold_dimmed() {
        // demo_game's newest play is KC (away): away hot, home cold.
        let g = demo_game();
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let mut hot = None;
        let mut cold = None;
        for y in 0..15u16 {
            for x in 0..49u16 {
                match buf[(x, y)].symbol() {
                    "▶" => hot = Some(buf[(x, y)].fg),
                    "◀" => cold = Some(buf[(x, y)].fg),
                    _ => {}
                }
            }
        }
        assert_eq!(hot.unwrap(), theme::rgb(g.away.color), "hot side in bright team color");
        assert_eq!(
            cold.unwrap(),
            theme::dimmed(theme::rgb(g.home.color)),
            "cold side still ticks, dimmed team color"
        );
    }

    fn mlb_game() -> Game {
        let mut g = demo_game();
        g.league = League::Mlb;
        g.period = "BOT 7TH".into();
        g.clock = String::new();
        g.situation = Some(Situation {
            down_distance: "2 OUT · 1-2".into(),
            balls: Some(1),
            strikes: Some(2),
            outs: Some(2),
            on_base: Some([true, false, false]),
            ..Default::default()
        });
        g.meter = Some(Meter::Diamond { occupied: [true, false, false] });
        g
    }

    fn nhl_game() -> Game {
        let mut g = demo_game();
        g.league = League::Nhl;
        g.period = "2ND".into();
        g.clock = "1:03".into();
        g.situation = None;
        g.home.abbr = "DAL".into();
        g.meter = Some(Meter::Penalty { team_abbr: "DAL".into(), seconds: 42 });
        g
    }

    /// One tile per meter kind, with the text its gauge row must END on —
    /// a row that clips loses exactly that tail first.
    fn metered_games() -> Vec<(&'static str, Game, &'static [&'static str])> {
        vec![
            ("RED ZONE", demo_game(), &["TO GOAL", "YD"]),
            ("LEAD", nba_game(None), &["TB +3"]),
            ("BASES", mlb_game(), &["1-2"]),
            ("PENALTY", nhl_game(), &["0:42"]),
        ]
    }

    /// Inner (border-stripped) rows of a rendered tile.
    fn inner_rows(buf: &ratatui::buffer::Buffer, w: u16, h: u16) -> Vec<String> {
        (1..h - 1)
            .map(|y| (1..w - 1).map(|x| buf[(x, y)].symbol().to_string()).collect())
            .collect()
    }

    fn row_with<'a>(rows: &'a [String], needle: &str) -> Option<(usize, &'a String)> {
        rows.iter().enumerate().find(|(_, r)| r.contains(needle))
    }

    #[test]
    fn diamond_row_shows_bases_outs_and_count_inline() {
        let buf = render_buffer(&mlb_game(), Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let rows = inner_rows(&buf, 49, 15);
        let (_, row) = row_with(&rows, "BASES").expect("BASES row");
        assert!(row.contains("◆◇◇"), "runner on first, second/third empty: {row:?}");
        assert!(row.contains("OUTS ●●○"), "two outs as dots: {row:?}");
        assert!(row.contains("COUNT 1-2"), "balls-strikes: {row:?}");
    }

    #[test]
    fn penalty_row_is_a_countdown_bar_with_the_clock() {
        let th = theme::current();
        let buf = render_buffer(&nhl_game(), Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let rows = inner_rows(&buf, 49, 15);
        let (_, row) = row_with(&rows, "PENALTY").expect("PENALTY row");
        assert!(row.contains("DAL"), "{row:?}");
        assert!(row.trim_end().ends_with("0:42"), "m:ss ends the row: {row:?}");
        let filled = row.matches('▮').count();
        let empty = row.matches('░').count();
        assert!(filled > 0 && empty > 0, "42s of a 2:00 minor is a partly drained bar: {row:?}");
        assert!(filled < empty, "42/120 left => more drained than filled: {row:?}");
        // The filled cells are the live color — a clock, not chrome.
        let y = rows.iter().position(|r| r.contains("PENALTY")).unwrap() as u16 + 1;
        let x = row.chars().position(|c| c == '▮').unwrap() as u16 + 1;
        assert_eq!(buf[(x, y)].fg, th.live);
    }

    #[test]
    fn meter_row_sits_under_identity_above_momentum_and_frees_the_right_column() {
        for (label, g, _) in metered_games() {
            let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
            let rows = inner_rows(&buf, 49, 15);
            let (meter_y, row) = row_with(&rows, label).unwrap_or_else(|| panic!("{label} row missing:\n{}", rows.join("\n")));
            let (mom_y, _) = row_with(&rows, "MOMENTUM").expect("momentum row");
            assert_eq!(meter_y, IDENTITY_H as usize, "{label} row directly under the identity block");
            assert_eq!(mom_y, meter_y + 1, "{label} row precedes momentum");
            assert!(row.starts_with(&format!(" {label}")), "{label} is the row's leading label: {row:?}");
            // The old right-hand column is gone: no vertical track anywhere.
            assert!(!rows.iter().any(|r| r.contains('┃')), "{label}: meter column survived:\n{}", rows.join("\n"));
        }
        // Plays now run the full inner width: the 47-cell tile shows text the
        // 38-cell column-era row cut off.
        let mut g = demo_game();
        g.last_plays[0].text = "Patrick Mahomes pass to T. Kelce for 3 yards (1st & Goal)".into();
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
        let rows = inner_rows(&buf, 49, 15);
        let (_, play) = row_with(&rows, "[1:27]").expect("play row");
        assert!(play.contains("T. Kelce"), "play line should reach the tile edge: {play:?}");
    }

    #[test]
    fn meter_rows_fit_every_tile_width_without_clipping() {
        // Every inner width a live mosaic produces (30..=60): the row keeps
        // its label at the left, its tail intact at the right, and a track
        // of at least METER_MIN_BAR cells between them.
        for (label, g, tails) in metered_games() {
            for inner_w in 30..=60u16 {
                let (w, h) = (inner_w + 2, 15);
                let buf = render_buffer(&g, Density::Standard, w, h, TileFx::default(), ScoreStyle::Big);
                let rows = inner_rows(&buf, w, h);
                let (_, row) = row_with(&rows, label)
                    .unwrap_or_else(|| panic!("{label} row missing at inner width {inner_w}:\n{}", rows.join("\n")));
                let trimmed = row.trim_end();
                assert!(
                    tails.iter().any(|t| trimmed.ends_with(t)),
                    "{label} at inner width {inner_w}: row must end on one of {tails:?}, got {row:?}"
                );
                assert!(row.chars().count() == inner_w as usize, "row is exactly the inner width");
                let track = row.chars().filter(|c| matches!(c, '━' | '─' | '●' | '┼' | '▮' | '░')).count();
                if label == "RED ZONE" || label == "LEAD" {
                    assert!(
                        track >= METER_MIN_BAR,
                        "{label} at inner width {inner_w}: track {track} < METER_MIN_BAR {METER_MIN_BAR}: {row:?}"
                    );
                    let marker = if label == "RED ZONE" { '●' } else { '▮' };
                    let m = row.find(marker).unwrap_or_else(|| panic!("{label} marker missing: {row:?}"));
                    let l = row.find(label).unwrap();
                    let t = tails.iter().filter_map(|t| row.rfind(t)).max().unwrap();
                    assert!(l < m && m < t, "{label} at {inner_w}: label < marker < tail: {row:?}");
                }
            }
        }
    }

    #[test]
    fn lead_marker_and_tag_take_the_leading_teams_color() {
        // DEN (away) 27-24 up in nba_game => plus_minus is home-away = -3.
        let mut g = nba_game(None);
        g.away_score = 88;
        g.home_score = 81;
        g.meter = Some(Meter::Lead { plus_minus: -7 });
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let rows = inner_rows(&buf, 49, 15);
        let (y, row) = row_with(&rows, "LEAD").expect("LEAD row");
        // The ends name the teams (away left, home right), not ±15: a signed
        // scale plotted "KC +7" on the minus half, which read as a bug.
        assert!(row.starts_with(" LEAD  KC ─"), "away abbr labels the left end: {row:?}");
        assert!(!row.contains("-15") && !row.contains("+15"), "no signed scale: {row:?}");
        assert!(row.trim_end().ends_with("KC +7"), "tag names the leader: {row:?}");
        let mx = row.chars().position(|c| c == '▮').expect("marker") as u16;
        let home_end = row.find(" TB ").expect("home abbr labels the right end");
        assert!((mx as usize) < home_end, "marker sits inside the track: {row:?}");
        let marker_fg = buf[(mx + 1, y as u16 + 1)].fg;
        assert_eq!(marker_fg, theme::rgb(g.away.color), "marker in the leading (away) team's color");
        // Marker sits left of center when the away team leads.
        let cx = row.chars().position(|c| c == '┼').expect("center tick");
        assert!((mx as usize) < cx, "away lead => marker left of center: {row:?}");

        g.meter = Some(Meter::Lead { plus_minus: 5 });
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let rows = inner_rows(&buf, 49, 15);
        let (y, row) = row_with(&rows, "LEAD").expect("LEAD row");
        assert!(row.trim_end().ends_with("TB +5"), "{row:?}");
        let mx = row.chars().position(|c| c == '▮').unwrap() as u16;
        assert_eq!(buf[(mx + 1, y as u16 + 1)].fg, theme::rgb(g.home.color));
        let cx = row.chars().position(|c| c == '┼').unwrap();
        assert!((mx as usize) > cx, "home lead => marker right of center: {row:?}");

        g.meter = Some(Meter::Lead { plus_minus: 0 });
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Compact);
        let rows = inner_rows(&buf, 49, 15);
        let (_, row) = row_with(&rows, "LEAD").expect("LEAD row");
        assert!(row.trim_end().ends_with("TIED"), "{row:?}");
    }

    #[test]
    fn missing_meter_omits_the_row_and_plays_gain_the_line() {
        let mut with = demo_game();
        for i in 0..6 {
            with.last_plays.push(Play {
                clock: format!("{i}:00"),
                team: "TB".into(),
                text: format!("play number {i}"),
                ..Default::default()
            });
        }
        let mut without = with.clone();
        without.meter = None;
        let plays = |g: &Game| {
            let buf = render_buffer(g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
            inner_rows(&buf, 49, 15).iter().filter(|r| r.contains('[')).count()
        };
        let (n_with, n_without) = (plays(&with), plays(&without));
        assert!(n_with >= 1, "metered tile still shows plays");
        assert_eq!(n_without, n_with + 1, "the omitted row goes to the plays feed");
        let buf = render_buffer(&without, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
        let rows = inner_rows(&buf, 49, 15);
        assert_eq!(row_with(&rows, "MOMENTUM").unwrap().0, IDENTITY_H as usize, "momentum moves up");
    }

    #[test]
    fn meter_labels_follow_section_label_discipline() {
        // broadcast grants section labels the league accent; studio is the
        // same palette with the grant withdrawn (muted). RED ZONE is the
        // earned live red in both.
        let label_fg = |theme_name: &str, g: &Game, label: &str| {
            theme::set_current(theme_name).unwrap();
            let buf = render_buffer(g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
            let rows = inner_rows(&buf, 49, 15);
            let (y, row) = row_with(&rows, label).expect("label row");
            // Cell index, not byte index: glyphs like ◆ sit ahead of OUTS.
            let x = row.char_indices().position(|(i, _)| row[i..].starts_with(label)).unwrap() as u16;
            let fg = buf[(x + 1, y as u16 + 1)].fg;
            theme::set_current("broadcast").unwrap();
            fg
        };
        let nba = nba_game(None);
        let bc = theme::builtin("broadcast");
        let st = theme::builtin("studio");
        assert!(bc.discipline.section_labels && !st.discipline.section_labels, "themes disagree as expected");
        assert_eq!(label_fg("broadcast", &nba, "LEAD"), bc.league_accent(League::Nba));
        assert_eq!(label_fg("studio", &nba, "LEAD"), st.muted);
        assert_eq!(label_fg("studio", &mlb_game(), "OUTS"), st.muted);
        assert_eq!(label_fg("studio", &nhl_game(), "PENALTY"), st.muted);
        assert_eq!(label_fg("studio", &demo_game(), "RED ZONE"), st.live);
    }

    #[test]
    fn soccer_tile_shows_the_match_minute_as_the_period() {
        let mut g = demo_game();
        g.league = League::Epl;
        g.period = "90'+3'".into();
        g.clock = String::new();
        g.situation = None;
        g.meter = None;
        let text = render_to_text(&g, Density::Standard, 49, 15);
        assert!(text.contains("90'+3'"), "match minute missing where other sports show period/clock:\n{text}");
    }

    #[test]
    fn big_three_digit_scores_never_panic_in_narrow_tiles() {
        // Regression: 120-118 in a 32-38 col tile indexed past the buffer
        // edge (tui-big-text renders the unclamped glyph width). Every
        // width/height a live mosaic can produce must render, panic-free.
        let mut g = demo_game();
        g.league = League::Nba;
        g.away_score = 120;
        g.home_score = 118;
        for w in 30..=60u16 {
            for h in [9, 12, 15] {
                let buf = render_buffer(&g, Density::Standard, w, h, TileFx::default(), ScoreStyle::Big);
                let text = buffer_text(&buf, w, h);
                assert!(
                    text.contains("120") || !text.is_empty(),
                    "tile {w}x{h} rendered:\n{text}"
                );
            }
        }
    }

    #[test]
    fn short_tile_fills_with_digits_names_momentum_and_plays() {
        // 80x24 four-up board: tiles are ~40x9 (inner 7 rows). That must be
        // a scoreboard — digits, names, momentum, at least one play — not a
        // header row floating over a void.
        let buf = render_buffer(&demo_game(), Density::Standard, 40, 9, TileFx::default(), ScoreStyle::Big);
        let text = buffer_text(&buf, 40, 9);
        assert!(!text.contains("27 - 24"), "short tile uses big digits, not the text row:\n{text}");
        assert!(text.contains("CHIEFS"), "names row missing:\n{text}");
        assert!(text.contains("RED ZONE") && text.contains("●"), "inline meter row missing:\n{text}");
        assert!(text.contains("MOMENTUM"), "momentum missing:\n{text}");
        assert!(text.contains("Mahomes"), "play line missing:\n{text}");
        // Order inside the 7 inner rows: digits(3) names meter momentum play.
        let rows = inner_rows(&buf, 40, 9);
        assert_eq!(row_with(&rows, "CHIEFS").unwrap().0, 3);
        assert_eq!(row_with(&rows, "RED ZONE").unwrap().0, 4);
        assert_eq!(row_with(&rows, "MOMENTUM").unwrap().0, 5);
        assert_eq!(row_with(&rows, "Mahomes").unwrap().0, 6);
    }

    #[test]
    fn focus_tile_body_shows_scoring_timeline_and_field_bar() {
        let mut g = demo_game();
        let td = crate::domain::Play {
            clock: "3:21".into(),
            team: "KC".into(),
            text: "Mahomes pass to Kelce, 12 yd TOUCHDOWN".into(),
            scoring: true,
            ..Default::default()
        };
        g.last_plays.push(td.clone());
        g.scoring_plays.push(td);
        let buf = render_buffer(&g, Density::Full, 118, 30, TileFx::default(), ScoreStyle::Big);
        let text = buffer_text(&buf, 118, 30);
        assert!(text.contains("LAST PLAYS"), "plays feed missing:\n{text}");
        assert!(text.contains("SCORING"), "scoring timeline missing:\n{text}");
        assert!(text.contains("TOUCHDOWN!"), "scoring word missing:\n{text}");
        assert!(text.contains("RED ZONE") && text.contains("3 TO GOAL"), "inline meter row missing:\n{text}");
        // In the red zone the gauge is the only ball plot: one marker, and
        // the row under MOMENTUM is a plain rule (focus.png had two dots for
        // one spot on two unlabeled scales).
        assert_eq!(text.matches('●').count(), 1, "one ball marker in the red zone:\n{text}");
        let mom = text.lines().position(|l| l.contains("MOMENTUM")).unwrap();
        let under = text.lines().nth(mom + 1).unwrap().trim_matches(|c| c == '│' || c == ' ');
        assert!(under.chars().all(|c| c == '─'), "plain rule under momentum, not a field bar: {under:?}");
        // Outside the red zone (no meter) the 100-yard field bar carries the drive.
        let mut mid = g.clone();
        mid.meter = None;
        mid.situation.as_mut().unwrap().ball_on = Some("KC 35".into());
        let text = buffer_text(&render_buffer(&mid, Density::Full, 118, 30, TileFx::default(), ScoreStyle::Big), 118, 30);
        assert!(text.contains(" G "), "field bar goal tag missing:\n{text}");
        assert_eq!(text.matches('●').count(), 1, "field bar marker:\n{text}");
        // The LED digits actually doubled: sextant "27" fits in 3 rows, the
        // full-size glyphs span 8 — count rows containing digit strokes.
        let stroke_rows = (0..30u16)
            .filter(|&y| (0..118u16).any(|x| buf[(x, y)].symbol() == "█"))
            .count();
        assert!(stroke_rows >= 6, "full-size digits should span >=6 rows, got {stroke_rows}");
    }

    #[test]
    fn name_row_records_are_a_pair_decision_and_never_truncate() {
        // 49 wide: "BUCCANEERS 11-6" (15) does not fit its 12-cell half.
        // "BUCCANEERS 1…" would read as a wrong record, so that side falls
        // back to "TB 11-6" — the record is worth more than the long name.
        // Whether records show at all is still a pair decision, so KC takes
        // the abbr form with it ("CHIEFS 11-6" beside a bare "BUCCANEERS",
        // board-broadcast.png, read as a missing record).
        let g = demo_game();
        let buf = render_buffer(&g, Density::Standard, 49, 15, TileFx::default(), ScoreStyle::Big);
        let text = buffer_text(&buf, 49, 15);
        let names = text.lines().find(|l| l.contains("TB 11-6")).expect("name row");
        assert!(names.contains("KC 11-6"), "both sides keep their record: {names:?}");
        assert!(!names.contains('…'), "abbr form, not an ellipsized name: {names:?}");
        // 60 wide: both pairs fit their 18-cell halves, both records show.
        let buf = render_buffer(&g, Density::Standard, 60, 15, TileFx::default(), ScoreStyle::Big);
        let text = buffer_text(&buf, 60, 15);
        let names = text.lines().find(|l| l.contains("CHIEFS")).expect("name row");
        assert!(names.contains("CHIEFS 11-6") && names.contains("BUCCANEERS 11-6"), "{names:?}");
    }

    #[test]
    fn compact_identity_city_line_is_a_pair_decision() {
        // 49 wide: each identity column is 9 cells — "TAMPA BAY" fits,
        // "KANSAS CITY" doesn't. board-compact.png showed "CHIEFS / 11-6"
        // over a blank city row beside "TAMPA BAY / TB / 11-6": the city
        // line must be both sides or neither.
        let g = demo_game();
        let text = render_to_text(&g, Density::Standard, 49, 15);
        assert!(!text.contains("TAMPA BAY") && !text.contains("KANSAS CITY"), "no city on either side:\n{text}");
        assert!(text.contains("CHIEFS") && text.contains("TB"), "name / abbr fallback still per side:\n{text}");
        // 62 wide: 15-cell columns — both cities fit, both show.
        let text = render_to_text(&g, Density::Standard, 62, 15);
        assert!(text.contains("TAMPA BAY") && text.contains("KANSAS CITY"), "both cities:\n{text}");
    }

    #[test]
    fn compact_identity_never_ellipsizes_city_or_name() {
        // Regression (board-compact.png): "KANSAS C…" / "BUCCANEE…" in the
        // 2x2 NFL card. A city that doesn't fit vanishes; a name that doesn't
        // fit becomes the abbr.
        let g = demo_game();
        let mut fell_back = false;
        for w in 40..=70u16 {
            let text = render_to_text(&g, Density::Standard, w, 15);
            for s in ["KANSAS CITY", "TAMPA BAY", "CHIEFS", "BUCCANEERS"] {
                // Prefixes of 4+ so a play line's own "…to K…" can't match.
                for k in 4..s.len() {
                    let cut = format!("{}…", &s[..k]);
                    assert!(!text.contains(&cut), "width {w} ellipsizes identity {cut:?}:\n{text}");
                }
            }
            if !text.contains("BUCCANEERS") {
                fell_back = true;
                assert!(text.contains("TB"), "width {w}: abbr fallback missing:\n{text}");
            }
        }
        assert!(fell_back, "some width in 40..=70 must be too narrow for BUCCANEERS");
    }

    #[test]
    fn compact_identity_always_gaps_score_from_names() {
        // Regression: "27 - 24BUCCANEERS" — the score column must carry a
        // spacer cell on both sides at every width.
        let g = demo_game();
        for w in 44..=70u16 {
            let buf = render_buffer(&g, Density::Standard, w, 15, TileFx::default(), ScoreStyle::Compact);
            let text = buffer_text(&buf, w, 15);
            let Some(row) = text.lines().find(|l| l.contains("27 - 24")) else {
                continue;
            };
            assert!(
                !row.contains("24B") && !row.contains("S27"),
                "score abuts a name at width {w}: {row:?}"
            );
        }
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
