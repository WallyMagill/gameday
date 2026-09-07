//! Zoomed single-game view (`z`/Enter): a tab bar — OVERVIEW │ PLAYS │ STATS
//! — over one game's full-body surface.
//!
//! Overview is the same [`hero`] block the board and `:tv` draw, the
//! shared [`linescore`] table, one per-sport matchup line, then the feed.
//! Plays is the game's full feed with a j/k highlight; Stats is the box score.

use crate::app::App;
use crate::board::{hero, linescore, rows};
use crate::domain::{EventKind, Extras, Game, HockeyStrength, League, Status};
use crate::text::truncate;
use crate::theme;
use crate::tiles;
use crate::views::ZoomTab;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(app: &mut App, frame: &mut Frame, area: Rect, game_id: &str, tab: ZoomTab) {
    let th = theme::current();
    let Some(game) = app.game_by_id(game_id) else {
        // The zoomed game left every board (final pruned, feed hiccup).
        frame.render_widget(
            Paragraph::new(format!("game {game_id:?} is not on any board · esc back"))
                .style(Style::default().fg(th.muted).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    draw_tab_bar(app, frame, chunks[0], &game, tab);
    match tab {
        ZoomTab::Overview => draw_overview(app, frame, chunks[1], &game),
        ZoomTab::Plays => draw_plays(app, frame, chunks[1], &game),
        ZoomTab::Stats => draw_stats(app, frame, chunks[1], &game),
    }
}

/// `OVERVIEW │ PLAYS │ STATS` — active tab in the same chip style as the
/// active league tab; matchup + score right-aligned for orientation. Each
/// label registers a click zone that switches to its tab.
fn draw_tab_bar(app: &mut App, frame: &mut Frame, area: Rect, game: &Game, active: ZoomTab) {
    let th = theme::current();
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in ZoomTab::ALL.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(th.dim)));
        }
        let style = if tab == active {
            Style::default()
                .fg(th.bg)
                .bg(th.star)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        let x: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let w = tab.label().chars().count();
        if x + w <= area.width as usize {
            app.hit_zones.push((
                Rect {
                    x: area.x + x as u16,
                    y: area.y,
                    width: w as u16,
                    height: 1,
                },
                crate::keymap::Hit::ZoomTab(tab),
            ));
        }
        spans.push(Span::styled(tab.label(), style));
    }
    let right = format!(
        "{} {} @ {} {} ",
        game.away.abbr, game.away_score, game.home.abbr, game.home_score
    );
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let spacer = (area.width as usize).saturating_sub(left_len + right.chars().count());
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.push(Span::styled(right, Style::default().fg(th.muted)));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(th.bg)),
        area,
    );
}

/// What the hero asks for in a zoom: the nameplate row, 8 rows of
/// `PixelSize::Full` digits (`tiles::glyph_cell().1`), and the three
/// optional rows the hero's own keep order can spend (fragment, meter, last
/// play) — `hero.rs`'s full budget. The zoom is the one surface with room to
/// grant all of it; a shorter pane falls through the hero's own ladder.
const HERO_ROWS: u16 = 1 + 8 + 3;

/// Rows the feed needs before the linescore and the matchup line may take
/// any: the dim rule, the LAST PLAYS label, and three plays. Below that the
/// section is a header over a void.
const FEED_MIN: u16 = 5;

/// The overview body, top down: hero, linescore, matchup line, feed.
/// Everything under the hero is charged against what the hero left, in
/// that order — the same "the hero shrinks last" rule the board runs on.
fn draw_overview(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    // The NHL penalty meter is the zoom's alone. `hero::draw_hero`
    // reads `game.meter`, the same field the board and `:tv` read, so the
    // meter is attached to a local copy here instead of being stored on the
    // shared game — the surfaces that must not show it read a field it was
    // never written to. The clone happens only for an NHL game actually on a
    // power play; every other game draws from the borrowed one.
    let with_meter;
    let game = match game.extras.penalty_meter() {
        Some(meter) if game.meter.is_none() => {
            with_meter = Game {
                meter: Some(meter),
                ..game.clone()
            };
            &with_meter
        }
        _ => game,
    };
    let th = theme::current();
    let hero_rows = HERO_ROWS.min(area.height);
    let mut rest = area.height - hero_rows;
    let linescore =
        linescore::linescore_lines(game, &th).filter(|_| rest >= linescore::ROWS + FEED_MIN);
    let ls_rows = if linescore.is_some() {
        linescore::ROWS
    } else {
        0
    };
    rest -= ls_rows;
    let matchup = matchup_line(app, game, area.width as usize).filter(|_| rest > FEED_MIN);
    let matchup_rows = u16::from(matchup.is_some());
    rest -= matchup_rows;

    let now = app.now();
    hero::draw_hero(
        frame,
        Rect {
            height: hero_rows,
            ..area
        },
        game,
        &hero::HeroPlan {
            // The zoom is a full-width surface, so it takes the board's own
            // >=100-col bracket for the big digits and the flanking marks.
            // The marks are hero-only, and TV, the cut and the row tiers are
            // the logo-free surfaces; the zoom IS the hero
            // block, at the width the marks were measured for.
            digits_full: area.width >= 100,
            chip: strength_chip(game).or_else(|| crate::rank::watchability(game, now).chip),
            now,
            pinned: app.pins.iter().any(|p| p.game_id == game.id),
            favorite: app.is_my_game(game),
            show_logos: area.width >= 100,
            // Nothing is selectable inside a zoom.
            selected: false,
        },
    );

    let mut y = area.y + hero_rows;
    if let Some(lines) = linescore {
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme::current().bg)),
            Rect {
                y,
                height: ls_rows,
                ..area
            },
        );
        y += ls_rows;
    }
    if let Some(line) = matchup {
        frame.render_widget(
            Paragraph::new(line).alignment(Alignment::Center),
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        y += matchup_rows;
    }
    if rest > 0 {
        draw_feed(
            frame,
            Rect {
                y,
                height: rest,
                ..area
            },
            game,
        );
    }
}

/// The NHL special-teams chip, from `Extras::Hockey` —
/// which only a summary fills, so only the zoomed game can wear it. The
/// board's own chip stays `rank::watchability`'s until the October
/// scoreboard probe says whether the scoreboard carries strength at all;
/// promotion is additive, this reads no scoreboard field.
///
/// It says POWER PLAY for both 702 and 703, and that is the honest reading
/// rather than a shortcut: ESPN's strength is relative to the acting play's
/// own team (the fixture stamps 702 on the advantaged side's plays and 703
/// on the penalized side's, inside the same two minutes), so the enum names
/// no side by itself. The zoom shows both teams at once and has no "shown
/// team" for SHORTHANDED to be relative to — the side serving it is named
/// by the penalty meter directly underneath, which carries the abbreviation.
fn strength_chip(game: &Game) -> Option<&'static str> {
    match &game.extras {
        Extras::Hockey {
            strength: HockeyStrength::PowerPlay | HockeyStrength::Shorthanded,
            ..
        } => Some("POWER PLAY"),
        _ => None,
    }
}

/// Match events shown on the soccer line. Three fits inside 80 columns with
/// surnames, and is the same count the play feeds elsewhere are given.
const MATCH_EVENTS: usize = 3;

/// Timeouts each side starts a half with (NFL and NBA rules). A feed value
/// above it simply draws that many filled pips rather than lying about the
/// total.
const TIMEOUTS_PER_HALF: u8 = 3;

/// The per-sport matchup/state line under the linescore. Every string here
/// comes from a field the mapper filled and nothing ever drew:
/// `Situation::{pitcher,batter,due_up}`, `Game::timeouts`, and
/// `Extras::Soccer::events`. `None` when the sport has no such line, or when
/// the feed has not filled the fields it would be made of.
///
/// A final gets no situation, no timeouts and no soccer events (ESPN clears
/// them once the game ends) — the per-league match below would always read
/// `None` for one, and a startup final (loaded already-final, no delta ever
/// captured) never printed a story anywhere in the zoom. So a final takes
/// this line over for its own header: the same final-story ladder,
/// `rows::final_story` shared with the board's tier-3 row so the two never
/// disagree.
fn matchup_line(app: &App, game: &Game, width: usize) -> Option<Line<'static>> {
    let th = theme::current();
    let r = th.roles();
    if game.status == Status::Final {
        let leaders = app.stats.get(&game.id).and_then(rows::leaders_line);
        let text = rows::final_story(game, leaders.as_deref())?;
        let span = Span::styled(truncate(&text, width), Style::default().fg(r.ink));
        return Some(Line::from(span));
    }
    let label = Style::default().fg(r.dim);
    let ink = Style::default().fg(r.ink).add_modifier(Modifier::BOLD);
    let sep = || Span::styled("  ·  ", Style::default().fg(r.dim));
    let team_color = |abbr: &str| {
        let (a, h, _) = theme::hero_pair(&th, game.away.color, game.home.color);
        if abbr.eq_ignore_ascii_case(&game.home.abbr) {
            h
        } else {
            a
        }
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    match game.league {
        League::Mlb => {
            let sit = game.situation.as_ref()?;
            for (tag, who) in [
                ("P: ", sit.pitcher.as_deref()),
                ("AB: ", sit.batter.as_deref()),
            ] {
                let Some(who) = who.filter(|s| !s.is_empty()) else {
                    continue;
                };
                if !spans.is_empty() {
                    spans.push(sep());
                }
                spans.push(Span::styled(tag, label));
                spans.push(Span::styled(who.to_string(), ink));
            }
            if !sit.due_up.is_empty() {
                if !spans.is_empty() {
                    spans.push(sep());
                }
                spans.push(Span::styled("DUE UP ", label));
                spans.push(Span::styled(
                    sit.due_up.join(", "),
                    Style::default().fg(r.ink),
                ));
            }
        }
        League::Nfl | League::Cfb | League::Nba | League::Wnba | League::Cbb => {
            let (away, home) = game.timeouts?;
            spans.push(Span::styled("TIMEOUTS ", label));
            spans.push(Span::styled(pips(away), Style::default().fg(r.ink)));
            spans.push(Span::styled(" │ ", Style::default().fg(r.dim)));
            spans.push(Span::styled(pips(home), Style::default().fg(r.ink)));
            // `KC BALL` is the hero fragment line's last phrase whenever the
            // hero has one (football), and the zoom's 12-row bracket always
            // draws it — printing it again three rows down read as two
            // different claims. So possession is this line's business only
            // for the sports the hero says nothing about (basketball).
            if let Some(poss) = hero::fragment_line(game)
                .is_none()
                .then(|| {
                    game.situation
                        .as_ref()
                        .and_then(|s| s.possession.as_deref())
                })
                .flatten()
                .filter(|s| !s.is_empty())
            {
                spans.push(sep());
                spans.push(Span::styled(
                    format!("{} BALL", poss.to_uppercase()),
                    Style::default()
                        .fg(team_color(poss))
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }
        League::Epl | League::Mls => {
            let Extras::Soccer { events, .. } = &game.extras else {
                return None;
            };
            if events.is_empty() {
                return None;
            }
            // Newest last, three at most: the line is the shape of the match,
            // not its log — the PLAYS tab has the whole thing.
            for ev in &events[events.len().saturating_sub(MATCH_EVENTS)..] {
                if !spans.is_empty() {
                    spans.push(sep());
                }
                spans.push(Span::styled(
                    format!("{} ", ev.minute),
                    Style::default().fg(th.clock()),
                ));
                spans.push(Span::styled(
                    format!("{} ", event_letter(ev.kind)),
                    Style::default()
                        .fg(team_color(&ev.team))
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(ev.player.clone(), Style::default().fg(r.ink)));
            }
        }
        League::Nhl => return None,
    }
    if spans.is_empty() {
        return None;
    }
    // Drop whole trailing spans rather than clip a name in half.
    let mut used = 0usize;
    let mut out: Vec<Span<'static>> = Vec::new();
    for span in spans {
        let w = span.content.chars().count();
        if used + w > width {
            break;
        }
        used += w;
        out.push(span);
    }
    (!out.is_empty()).then(|| Line::from(out))
}

/// Terminal-legal event letters — never emoji, which are double-width on
/// some terminals and blank on others (the global terminal rule).
fn event_letter(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Goal => "G",
        EventKind::OwnGoal => "OG",
        EventKind::Penalty => "PEN",
        EventKind::Yellow => "Y",
        EventKind::Red => "R",
        EventKind::Sub => "SUB",
    }
}

/// Timeouts remaining as filled/empty pips against [`TIMEOUTS_PER_HALF`].
fn pips(left: u8) -> String {
    (0..left.max(TIMEOUTS_PER_HALF))
        .map(|i| if i < left { '●' } else { '○' })
        .collect()
}

/// Rows a feed section keeps even when the other is long: a caption over
/// fewer than four rows is a header over a void (the old `FEED_MIN` said
/// the same about the whole feed).
const FEED_FLOOR: usize = 4;

/// Split `avail` body rows (the two rules and two captions already taken)
/// between LAST PLAYS and SCORING: in proportion to what each has, each
/// floored at [`FEED_FLOOR`] when it has that many, and rows one section
/// cannot use go to the other — the pane fills whenever the game has the
/// plays to fill it (L5 was five to eight blank rows under SCORING). Under
/// two floors (`avail` too small to grant both), neither floor can hold, so
/// this falls back to a plain proportional split rounded to the nearest
/// row — except a section that exists (its input is at least one) never
/// drops to zero while `avail >= 2` leaves room to steal a row from the
/// other side. At `avail == 1` the lone row goes to LAST PLAYS and at
/// `avail == 0` neither section gets one: both are the ties `draw_feed`
/// (round 1 fix) now honors strictly, since SCORING draws last and would
/// otherwise be the section `lines.truncate` silently ate.
pub(crate) fn feed_split(avail: usize, plays: usize, scoring: usize) -> (usize, usize) {
    if plays + scoring <= avail {
        return (plays, scoring);
    }
    let floor_p = FEED_FLOOR.min(plays);
    let floor_s = FEED_FLOOR.min(scoring);
    if avail < floor_p + floor_s {
        if avail == 0 {
            return (0, 0);
        }
        if avail == 1 {
            return (1, 0);
        }
        let total = plays + scoring;
        let mut p = (2 * avail * plays + total) / (2 * total);
        p = p.min(avail).min(plays);
        let mut s = avail.saturating_sub(p).min(scoring);
        if p == 0 && plays >= 1 {
            p = 1;
            s = avail - p;
        } else if s == 0 && scoring >= 1 {
            s = 1;
            p = avail - s;
        }
        return (p, s);
    }
    let mut p = (avail * plays / (plays + scoring)).max(floor_p).min(plays);
    let mut s = avail.saturating_sub(p).min(scoring);
    if s < floor_s {
        s = floor_s.min(avail);
        p = avail.saturating_sub(s).min(plays);
    }
    if p + s < avail {
        p = avail.saturating_sub(s).min(plays);
    }
    if p + s < avail {
        s = avail.saturating_sub(p).min(scoring);
    }
    (p, s)
}

/// LAST PLAYS over the game's feed, then SCORING over `game.scoring_plays` —
/// the two sections the old focus tile ended with, unchanged in content.
fn draw_feed(frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let r = th.roles();
    let width = area.width as usize;
    // Two rules and two captions are fixed; what's left splits between the
    // sections in proportion to what each has (an empty section still
    // prints its one-line message, so it counts one).
    let avail = (area.height as usize).saturating_sub(4);
    let (p_rows, s_rows) = feed_split(
        avail,
        game.last_plays.len().max(1),
        game.scoring_plays.len().max(1),
    );
    let rule = || {
        Line::from(Span::styled(
            "─".repeat(width.saturating_sub(2)),
            Style::default().fg(r.cool),
        ))
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    // The split is honored strictly: a section allotted zero rows draws
    // nothing at all — no rule, no caption, no empty-message placeholder —
    // so a caption can never sit over a void, and SCORING (drawn last)
    // can never be the tail `lines.truncate` silently eats because an
    // unbudgeted line snuck in ahead of it.
    if p_rows > 0 {
        lines.push(rule());
        lines.push(Line::from(Span::styled(
            " LAST PLAYS",
            Style::default()
                .fg(th.section_label(th.league_accent(game.league)))
                .add_modifier(Modifier::BOLD),
        )));
        if game.last_plays.is_empty() {
            // A pre-game zoom has no plays; its line is the betting line
            // (dim — odds are context, never chrome-loud).
            let empty = match (&game.status, &game.odds) {
                (Status::Pre, Some(odds)) => format!(" {odds}"),
                _ => " no plays yet".to_string(),
            };
            lines.push(Line::from(Span::styled(empty, Style::default().fg(r.dim))));
        } else {
            lines.extend(
                game.last_plays
                    .iter()
                    .take(p_rows)
                    .map(|p| tiles::play_line(game, p, width)),
            );
        }
    }
    if s_rows > 0 {
        lines.push(rule());
        lines.push(Line::from(Span::styled(
            " SCORING",
            Style::default().fg(r.hot).add_modifier(Modifier::BOLD),
        )));
        if game.scoring_plays.is_empty() {
            lines.push(Line::from(Span::styled(
                " no scoring yet",
                Style::default().fg(r.dim),
            )));
        } else {
            let word = theme::scoring_word(game.league);
            for p in game.scoring_plays.iter().rev().take(s_rows) {
                let color = if p.team.eq_ignore_ascii_case(&game.away.abbr) {
                    th.team_text(game.away.color)
                } else {
                    th.team_text(game.home.color)
                };
                let head = format!(" [{}] {:<3} ", tiles::play_stamp(p), p.team);
                let used = head.chars().count() + word.chars().count() + 1;
                lines.push(Line::from(vec![
                    Span::styled(
                        head,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{word} "),
                        Style::default().fg(r.hot).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        truncate(
                            tiles::without_leading_clock(&p.text, !p.clock.is_empty()),
                            width.saturating_sub(used + 1),
                        ),
                        Style::default().fg(r.ink),
                    ),
                ]));
            }
        }
    }
    // The last resort only: the split above already keeps every drawn
    // section inside its budget, so this never fires in practice.
    lines.truncate(area.height as usize);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        area,
    );
}

/// Full play feed for this game (its `last_plays`, newest first as mapped);
/// `app.zoom_scroll` is the highlighted row (j/k or the mouse wheel), kept
/// on screen by a simple scroll window.
fn draw_plays(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    if game.last_plays.is_empty() {
        frame.render_widget(
            Paragraph::new("no plays yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let sel = app.zoom_scroll.min(game.last_plays.len() - 1);
    // Keep the highlight visible: scroll the window once it walks past the
    // bottom row.
    let visible = area.height.max(1) as usize;
    let skip = sel.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = game
        .last_plays
        .iter()
        .enumerate()
        .skip(skip)
        .take(visible)
        .map(|(i, play)| {
            let marker = if i == sel { "▸ " } else { "  " };
            let text_style = if play.scoring {
                Style::default().fg(th.live).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(th.fg)
            };
            let mut spans = vec![
                Span::styled(marker, Style::default().fg(th.star)),
                Span::styled(
                    format!("{:>5} ", play.clock),
                    Style::default().fg(th.clock()),
                ),
                Span::styled(
                    format!("{:<4}", play.team),
                    Style::default()
                        .fg(App::team_color(game, &play.team))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(play.text.clone(), text_style),
            ];
            if i == sel {
                spans[3] = spans[3]
                    .clone()
                    .style(text_style.add_modifier(Modifier::BOLD));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        area,
    );
}

/// Value columns are sized to the widest value ("6-17", "31:26"), floored at
/// the 3-char abbr header width plus a space.
const STAT_COL_MIN: usize = 4;

/// Box score: comparison rows (label + away/home value columns under the team
/// abbrs) scrolled by j/k, with the LEADERS block pinned below. Empty until
/// the ~30s stats poll answers — says so instead of rendering a blank pane.
fn draw_stats(app: &App, frame: &mut Frame, area: Rect, game: &Game) {
    let th = theme::current();
    let stats = app.stats.get(&game.id);
    let Some(stats) = stats.filter(|s| !s.rows.is_empty() || !s.leaders.is_empty()) else {
        frame.render_widget(
            Paragraph::new("no stats yet")
                .style(Style::default().fg(th.dim).bg(th.bg))
                .alignment(Alignment::Center),
            area,
        );
        return;
    };
    // LEADERS gets its rows plus a header, but never more than half the pane;
    // the comparison table keeps the rest.
    let leaders_h = if stats.leaders.is_empty() {
        0
    } else {
        (stats.leaders.len() as u16 + 2).min(area.height / 2)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(leaders_h)])
        .split(area);

    let col_w = stats
        .rows
        .iter()
        .flat_map(|r| [r.away.chars().count(), r.home.chars().count()])
        .max()
        .unwrap_or(0)
        .max(STAT_COL_MIN);
    // Label column hugs the widest label instead of stretching to the pane
    // edge — a 120-col pane would otherwise put ~70 blank cells between a
    // label and its values.
    let widest_label = stats
        .rows
        .iter()
        .map(|r| r.label.chars().count())
        .max()
        .unwrap_or(0);
    let label_w = widest_label.min((chunks[0].width as usize).saturating_sub(2 * (col_w + 2) + 3));
    let mut lines = vec![Line::from(vec![
        Span::raw(" ".repeat(label_w + 3)),
        Span::styled(
            format!("{:>col_w$}", game.away.abbr),
            Style::default()
                .fg(theme::rgb(game.away.color))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:>col_w$}", game.home.abbr),
            Style::default()
                .fg(theme::rgb(game.home.color))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    // j/k move a highlight through the rows; the window follows it, minus the
    // abbr header line.
    let sel = app.zoom_scroll.min(stats.rows.len().saturating_sub(1));
    let visible = (chunks[0].height.max(1) as usize).saturating_sub(1).max(1);
    let skip = sel.saturating_sub(visible.saturating_sub(1));
    for (i, row) in stats.rows.iter().enumerate().skip(skip).take(visible) {
        let marker = if i == sel { "▸ " } else { "  " };
        let label_style = if i == sel {
            Style::default().fg(th.bright).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(th.muted)
        };
        let mut label: String = row.label.chars().take(label_w).collect();
        let pad = label_w.saturating_sub(label.chars().count());
        label.push_str(&" ".repeat(pad));
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(th.star)),
            Span::styled(label, label_style),
            Span::raw(" "),
            Span::styled(format!("{:>col_w$}", row.away), Style::default().fg(th.fg)),
            Span::raw("  "),
            Span::styled(format!("{:>col_w$}", row.home), Style::default().fg(th.fg)),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(th.bg)),
        chunks[0],
    );

    if leaders_h > 0 {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " LEADERS",
                Style::default()
                    .fg(th.section_label(th.star))
                    .add_modifier(Modifier::BOLD),
            )),
        ];
        let label_w = stats
            .leaders
            .iter()
            .map(|l| l.label.chars().count())
            .max()
            .unwrap_or(0);
        for leader in &stats.leaders {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "  {:<w$}",
                        leader.team,
                        w = crate::board::rows::ABBR_W as usize
                    ),
                    Style::default()
                        .fg(App::team_color(game, &leader.team))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<label_w$}  ", leader.label.to_uppercase()),
                    Style::default().fg(th.muted),
                ),
                Span::styled(leader.text.clone(), Style::default().fg(th.fg)),
            ]));
        }
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(th.bg)),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::feed_split;

    #[test]
    fn feed_split_shares_the_pane_proportionally_with_a_floor_of_four() {
        assert_eq!(feed_split(20, 6, 6), (6, 6), "fits: nothing to share");
        assert_eq!(
            feed_split(20, 40, 10),
            (16, 4),
            "proportional, scoring at its floor"
        );
        assert_eq!(
            feed_split(20, 40, 2),
            (18, 2),
            "a section never gets more than it has"
        );
        assert_eq!(
            feed_split(20, 3, 40),
            (3, 17),
            "the other section takes the leftover"
        );
        assert_eq!(feed_split(8, 40, 40), (4, 4), "both at the floor");
        assert_eq!(
            feed_split(3, 40, 40),
            (2, 1),
            "under two floors: proportional, scoring last"
        );
    }

    /// Round 1 fix: a section that exists (its input is at least one) never
    /// starves to zero rows just because it lost the floor race — the
    /// review that prompted this found `draw_feed` still printing that
    /// section's caption and one-line message even when its budget was
    /// zero, so the vector overdrew and `truncate` ate SCORING's tail.
    #[test]
    fn feed_split_keeps_a_section_that_exists_alive_under_two_floors() {
        assert_eq!(
            feed_split(3, 1, 10),
            (1, 2),
            "LAST PLAYS exists and keeps its one row, stolen from SCORING"
        );
        assert_eq!(
            feed_split(2, 1, 10),
            (1, 1),
            "exactly enough for one row each"
        );
        assert_eq!(
            feed_split(1, 1, 10),
            (1, 0),
            "the lone row goes to LAST PLAYS — SCORING draws last"
        );
        assert_eq!(feed_split(0, 10, 10), (0, 0), "no room, no rows at all");
        assert_eq!(
            feed_split(5, 40, 1),
            (4, 1),
            "SCORING's one real row keeps it, at its own floor of one"
        );
    }
}
