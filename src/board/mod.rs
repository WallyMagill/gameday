//! The v3.2 board: one ranked list, cut into sections (spec §1).
//!
//! Everything on screen below the header is a *block* — a section rule, the
//! hero, or a tier-1/2/3 row — stacked in `Derived::selection` order:
//!
//! ```text
//! MY GAMES ───────────────────── 2 PINNED · NEVER RE-SORTS
//!   <hero>                      (when the band's top game is live)
//!   <band rows>
//! IN PLAY ─────────────────────── SORTED BY WATCHABILITY
//!   <tier 1 blocks>  <tier 2 rows>
//! FINAL ───
//!   <tier 3 rows>
//! LATER ───
//!   <tier 3 rows>
//! SCORES  …off-screen games…     (only when something didn't fit)
//! ```
//!
//! Three things this module owns and nothing else does:
//!
//! * **The window.** The block list is always built whole; the view is a
//!   window onto it that scrolls by WHOLE blocks so the hero can never be
//!   drawn with its top three rows missing. The window is a pure function of
//!   `selected` — no scroll offset is stored, so a board that shrinks between
//!   frames cannot strand the selection off-screen.
//! * **The tier promotion.** [`layout::plan`] says how many live games are
//!   promoted to 3-row tier-1 blocks and how tall the hero is; which games
//!   those are is the ranked order's business, so it is simply the top N.
//! * **The lane.** `plan.scores_lane` decides the *budget* (Task 5); what is
//!   actually off-screen is only knowable after the window is built, so the
//!   lane's presence and content come from the window, and the lane names
//!   off-screen LIVE games first — a lane fired by LATER truncation alone
//!   (Task 5's note) says `2 OFF-SCREEN · 2 FINAL · 4 LATER` instead.
//!
//! No borders anywhere: a section is a label, a dim rule, and a right-hand
//! caption (spec §1, the A′ frames).

pub mod cut;
pub mod hero;
pub mod layout;
pub mod linescore;
pub mod logo;
pub mod rows;

use crate::app::net::NetChip;
use crate::app::{App, Tab};
use crate::domain::{Game, Status};
use crate::rank;
use crate::theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

/// Rows a tier-1 block occupies: the nameplate row, the fragment/play row
/// under it, and one row of air (`rows::draw_tier1`). `rows` draws its gutter
/// marks down exactly this many rows, so the number lives here once.
pub(crate) const TIER1_ROWS: u16 = 3;

/// One drawable unit of the board. Every block is a whole number of rows and
/// is drawn or skipped as a unit; only the game blocks are selectable.
enum Block<'a> {
    /// Section rule: label, dim dashes, right-hand caption.
    Rule(&'static str, String),
    /// The hero, `rows` tall (from [`layout::TierPlan::hero_rows`]).
    Hero(&'a Game, usize, u16),
    Tier1(&'a Game, usize),
    Tier2(&'a Game, usize),
    Tier3(&'a Game, usize),
}

impl Block<'_> {
    fn rows(&self) -> u16 {
        match self {
            Block::Rule(..) | Block::Tier2(..) | Block::Tier3(..) => 1,
            Block::Hero(_, _, rows) => *rows,
            Block::Tier1(..) => TIER1_ROWS,
        }
    }

    /// Index into `Derived::selection`, for the blocks j/k can land on.
    fn selection_index(&self) -> Option<usize> {
        match self {
            Block::Rule(..) => None,
            Block::Hero(_, i, _) | Block::Tier1(_, i) | Block::Tier2(_, i) | Block::Tier3(_, i) => {
                Some(*i)
            }
        }
    }

}

pub fn draw(app: &mut App, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if draw_empty_state(app, frame, area) {
        return;
    }
    let now = app.now();
    let tick = app.tick;
    let selected = app.selected;
    // The whole walk borrows `app` through `derived()`; the click zones it
    // collects can only be written back once that borrow has ended.
    let zones = board_walk(app, frame, area, now, tick, selected);
    app.hit_zones.extend(zones);
}

fn board_walk<'a>(
    app: &'a App,
    frame: &mut Frame,
    area: Rect,
    now: time::OffsetDateTime,
    tick: u64,
    selected: usize,
) -> Vec<(Rect, crate::keymap::Hit)> {
    let d = app.derived();

    // The band's row cost excludes the hero even when the hero IS a band game
    // — `layout::plan` counts the hero separately and says double-charging it
    // is the caller's bug.
    let hero_id = d.hero_id.clone();
    let hero_in_band = d
        .my_games
        .iter()
        .any(|g| Some(&g.id) == hero_id.as_ref());
    let band_rows = d.my_games.len() - usize::from(hero_in_band);
    let plan = layout::plan(
        area.width,
        area.height,
        d.in_play.len(),
        d.finals.len(),
        d.later.len(),
        band_rows,
    );

    // ---------------------------------------------------------- the blocks
    let mut blocks: Vec<Block> = Vec::new();
    let mut index = 0usize; // walks Derived::selection in the same order

    let hero_block =
        |game: &'a Game, i: usize| -> Block<'a> { Block::Hero(game, i, plan.hero_rows) };
    if !d.my_games.is_empty() {
        let pinned = d
            .my_games
            .iter()
            .filter(|g| app_pins_hold(app, g))
            .count();
        let caption = if pinned > 0 {
            format!("{pinned} PINNED · NEVER RE-SORTS")
        } else {
            "NEVER RE-SORTS".to_string()
        };
        blocks.push(Block::Rule("MY GAMES", caption));
    }
    for game in &d.my_games {
        if Some(&game.id) == hero_id.as_ref() && plan.hero_rows > 0 {
            blocks.push(hero_block(game, index));
        } else if game.status == Status::Live {
            blocks.push(Block::Tier2(game, index));
        } else {
            blocks.push(Block::Tier3(game, index));
        }
        index += 1;
    }

    // The hero of a board with no band sits above the IN PLAY rule: it is the
    // headline, not the first row of a section.
    let in_play_hero = if hero_in_band || plan.hero_rows == 0 {
        None
    } else {
        d.in_play
            .iter()
            .position(|g| Some(&g.id) == hero_id.as_ref())
    };
    if let Some(at) = in_play_hero {
        blocks.push(hero_block(&d.in_play[at], index + at));
    }
    if !d.in_play.is_empty() {
        blocks.push(Block::Rule(
            "IN PLAY",
            format!("SORTED BY {}", sort_phrase(app.config.sort)),
        ));
    }
    let mut promoted = 0usize;
    for (i, game) in d.in_play.iter().enumerate() {
        if in_play_hero == Some(i) {
            continue; // already drawn above the rule
        }
        if promoted < plan.tier1 {
            promoted += 1;
            blocks.push(Block::Tier1(game, index + i));
        } else {
            blocks.push(Block::Tier2(game, index + i));
        }
    }
    index += d.in_play.len();

    for (label, games) in [("FINAL", &d.finals), ("LATER", &d.later)] {
        if games.is_empty() {
            continue;
        }
        blocks.push(Block::Rule(label, String::new()));
        for (i, game) in games.iter().enumerate() {
            blocks.push(Block::Tier3(game, index + i));
        }
        index += games.len();
    }

    // ------------------------------------------------------ the reservation
    // spec §3: the band's two rows belong to the board whether or not a band
    // is firing (`plan.band_rows`) — `App::draw` draws the band straight into
    // them, over whatever is there, so nothing below ever moves.
    //
    // They are not two blank lines: the board's top section rule moves up
    // into the reservation, leaving one row of air under it, and the list
    // below starts at the reservation's bottom. That rule is then out of the
    // window entirely, so the reservation's real cost to the list is one row,
    // not two. A firing band covers the rule for its three seconds — a label
    // is the cheapest thing on the board to spend, and the alternative (the
    // band over the hero's nameplate, or the list) is worse.
    //
    // A board whose first block is the hero (no MY GAMES band: the hero sits
    // above the IN PLAY rule as the headline) has no rule to hoist, so its
    // reservation is air above the headline.
    let hoisted = if plan.band_rows > 0 && matches!(blocks.first(), Some(Block::Rule(..))) {
        Some(blocks.remove(0))
    } else {
        None
    };
    let body = Rect {
        y: area.y + plan.band_rows,
        height: area.height - plan.band_rows,
        ..area
    };

    // ---------------------------------------------------------- the window
    // The lane costs a row, so it is decided before the window is measured:
    // anything that does not fit the body means a lane, and the lane's row
    // comes off the body. (`plan.scores_lane` says the same thing about the
    // budget; the window is what actually knows.)
    let total: u16 = blocks.iter().map(|b| b.rows()).sum();
    let lane = total > body.height;
    let window = body.height - u16::from(lane);
    let first = first_visible(&blocks, selected, window);

    // The hoisted rule is a header for the list's top, so it is drawn only
    // while that top is actually on screen — a scrolled board would otherwise
    // caption rows from a different section.
    if let (Some(Block::Rule(label, caption)), 0) = (&hoisted, first) {
        draw_rule(
            frame,
            Rect { x: area.x, y: area.y, width: area.width, height: layout::RULE_ROWS },
            label,
            caption,
        );
    }

    let mut y = 0u16;
    let mut drawn: Vec<usize> = Vec::new();
    let mut zones: Vec<(Rect, crate::keymap::Hit)> = Vec::new();
    let visible = &blocks[first..];
    for (i, block) in visible.iter().enumerate() {
        let rows = block.rows();
        if y + rows > window {
            break;
        }
        // spec v3.3 §4: a rule that fits is still an orphan if the window
        // runs out immediately after it — the truncation path, not just the
        // empty-list one (`if games.is_empty() { continue; }` above only
        // catches a section with zero games). Distinguish that from IN
        // PLAY's legitimate zero-content case (the section's only game is
        // the hero, drawn above the rule, so no tier1/tier2 block follows
        // it in `blocks` at all — that rule stands on its own by design,
        // never suppressed). Only when a real content block for this
        // section *exists* right after the rule, but doesn't fit the
        // window, is the rule an orphan — skip it and stop, since nothing
        // after it fits either (`y` only grows) and the SCORES lane already
        // accounts for every row that didn't make the window.
        if matches!(block, Block::Rule(..)) {
            if let Some(next) = visible.get(i + 1) {
                if !matches!(next, Block::Rule(..)) && y + rows + next.rows() > window {
                    break;
                }
            }
        }
        let rect = Rect {
            x: body.x,
            y: body.y + y,
            width: body.width,
            height: rows,
        };
        match block {
            Block::Rule(label, caption) => draw_rule(frame, rect, label, caption),
            Block::Hero(game, i, _) => {
                let watch = rank::watchability(game, now);
                hero::draw_hero(
                    frame,
                    rect,
                    game,
                    &hero::HeroPlan {
                        digits_full: plan.hero_digits_full,
                        chip: watch.chip,
                        now,
                        pinned: app_pins_hold(app, game),
                        favorite: app.is_my_game(game),
                        // Under 100 columns the flanks are the first casualty
                        // (spec §4) — never the digits.
                        show_logos: area.width >= 100,
                        // The hero is a selectable row (task-9 review carry
                        // forward #1): a `▸` on the nameplates, like the
                        // caret every other selected row gets in its gutter.
                        selected: *i == selected,
                    },
                );
            }
            Block::Tier1(game, i) | Block::Tier2(game, i) | Block::Tier3(game, i) => {
                let watch = rank::watchability(game, now);
                let ctx = rows::RowCtx {
                    hot: watch.hot,
                    chip: watch.chip,
                    nudge: app.order.nudge(&game.id, tick),
                    selected: *i == selected,
                    pinned: app_pins_hold(app, game),
                    league_tag: d.mixed,
                    now,
                };
                match block {
                    Block::Tier1(..) => rows::draw_tier1(frame, rect, game, &ctx),
                    Block::Tier2(..) => rows::draw_tier2(frame, rect, game, &ctx),
                    _ => rows::draw_tier3(frame, rect, game, &ctx),
                }
            }
        }
        if let Some(i) = block.selection_index() {
            drawn.push(i);
            zones.push((rect, crate::keymap::Hit::Row(i)));
        }
        y += rows;
    }

    if lane {
        let off: Vec<&Game> = d
            .selection
            .iter()
            .enumerate()
            .filter(|(i, _)| !drawn.contains(i))
            .map(|(_, g)| g)
            .collect();
        draw_lane(
            frame,
            Rect {
                x: area.x,
                y: area.y + area.height - 1,
                width: area.width,
                height: 1,
            },
            &off,
        );
    }
    zones
}

/// True while `game` is pinned (as opposed to merely favorited): only a pin
/// colors an abbr and counts in the band's caption.
fn app_pins_hold(app: &App, game: &Game) -> bool {
    app.pins.iter().any(|p| p.game_id == game.id)
}

/// The A′ rule's right-hand caption: `SORTED BY WATCHABILITY`. The sort keys
/// are named for the footer/header in two words; the rule says the long form
/// of the default because that is what the frame prints.
fn sort_phrase(key: rank::SortKey) -> &'static str {
    match key {
        rank::SortKey::Watch => "WATCHABILITY",
        other => other.label(),
    }
}

/// The first block to draw so that the selected block is fully inside a
/// `window`-row view. Walks forward one block at a time — the list is at most
/// a few dozen blocks, so an O(n²) worst case here is cheaper than carrying a
/// scroll offset that a resize could invalidate.
fn first_visible(blocks: &[Block], selected: usize, window: u16) -> usize {
    let Some(target) = blocks
        .iter()
        .position(|b| b.selection_index() == Some(selected))
    else {
        return 0;
    };
    let mut first = 0usize;
    loop {
        let mut used = 0u16;
        let mut last = None;
        for (i, b) in blocks.iter().enumerate().skip(first) {
            if used + b.rows() > window {
                break;
            }
            used += b.rows();
            last = Some(i);
        }
        match last {
            Some(last) if target <= last => return first,
            // Nothing fits at all (a window shorter than one block): give up
            // rather than loop off the end.
            None => return first.min(target),
            _ => first += 1,
        }
    }
}

/// `IN PLAY ─────── SORTED BY WATCHABILITY`: a label, dim dashes, a caption.
/// No box drawing — the rule IS the section's only structure (spec §1).
fn draw_rule(frame: &mut Frame, area: Rect, label: &str, caption: &str) {
    let r = theme::current().roles();
    let w = area.width as usize;
    let label_w = label.chars().count();
    let caption_w = caption.chars().count();
    // One space of air on each side of the dashes; a rule too narrow for any
    // dash just prints the label.
    let dashes = w
        .saturating_sub(label_w + caption_w + if caption.is_empty() { 2 } else { 3 });
    let mut spans = vec![
        Span::styled(
            label.to_string(),
            Style::default().fg(r.cool).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("─".repeat(dashes), Style::default().fg(r.dim)),
    ];
    if !caption.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(caption.to_string(), Style::default().fg(r.dim)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The off-screen lane (spec §1). Live games first, by name and score,
/// because a game you can't see but is playing is the only thing the lane is
/// for; when nothing live is off-screen it degrades to the counts.
fn draw_lane(frame: &mut Frame, area: Rect, off: &[&Game]) {
    if off.is_empty() {
        return;
    }
    let r = theme::current().roles();
    let live: Vec<&&Game> = off.iter().filter(|g| g.status == Status::Live).collect();
    let body = if live.is_empty() {
        let finals = off.iter().filter(|g| g.status == Status::Final).count();
        let later = off.iter().filter(|g| g.status == Status::Pre).count();
        format!("{} OFF-SCREEN · {finals} FINAL · {later} LATER", off.len())
    } else {
        live.iter()
            .map(|g| {
                format!(
                    "{} {} {} {}",
                    g.away.abbr, g.away_score, g.home.abbr, g.home_score
                )
            })
            .collect::<Vec<_>>()
            .join("  ·  ")
    };
    let room = (area.width as usize).saturating_sub(9);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "SCORES  ",
                Style::default().fg(r.cool).add_modifier(Modifier::BOLD),
            ),
            Span::styled(crate::text::truncate(&body, room), Style::default().fg(r.dim)),
        ])),
        area,
    );
}

/// A two-line block sitting on the vertical middle of `area` — the empty
/// board's message reads as a centered statement, not a top-left log line.
fn center_two_lines(area: Rect) -> Rect {
    let h = 2u16.min(area.height);
    Rect {
        y: area.y + (area.height.saturating_sub(h)) / 2,
        height: h,
        ..area
    }
}

/// The empty-board branches, moved verbatim from v3.1's mosaic (the strings
/// are pinned by tests and must not change). Returns true when it drew one
/// and the board itself must not.
fn draw_empty_state(app: &mut App, frame: &mut Frame, area: Rect) -> bool {
    let th = theme::current();
    let net = app.net.chip(std::time::Instant::now(), app.stale_after());
    let empty = app.derived().selection.is_empty();
    match app.tab {
        // An active filter that matches nothing names the pattern, the scope
        // it searched and where the pattern IS live, instead of pretending
        // the board is empty. It wraps: the scope is the whole point of the
        // message, so a narrow board must never chop it off.
        _ if empty && app.active_filter().is_some() => {
            frame.render_widget(
                Paragraph::new(app.filter_miss_message())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            true
        }
        // An empty board during an outage must never read as "no games
        // tonight": name the outage and the error that caused it.
        _ if empty && matches!(net, NetChip::Offline { .. } | NetChip::NoDataYet) => {
            let detail = match &net {
                NetChip::Offline { error, .. } => format!("last error: {error}"),
                _ => "waiting for the first scoreboard…".to_string(),
            };
            let headline = net.label().unwrap_or_default();
            let color = if matches!(net, NetChip::Offline { .. }) {
                th.live
            } else {
                th.muted
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        headline,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(detail, Style::default().fg(th.muted))),
                ])
                .style(Style::default().bg(th.bg))
                .alignment(Alignment::Center),
                center_two_lines(area),
            );
            true
        }
        // Home carries every live game AND every scheduled/final one now
        // (spec §1: Home is whole-day) — an empty Home means nothing is
        // scheduled at all today, not merely nothing live. The old "nothing
        // live · next: …" message for a still-empty board with an upcoming
        // game was dead: any upcoming game is itself a LATER entry in
        // `selection`, which makes the board non-empty (task-9 review
        // carry-forward #2) — so only the true-empty message remains.
        Tab::Home if empty => {
            frame.render_widget(
                Paragraph::new("nothing live on the enabled boards · :config to add leagues")
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            true
        }
        Tab::League(_) if empty => {
            frame.render_widget(
                Paragraph::new("next kickoff")
                    .style(Style::default().fg(th.muted).bg(th.bg))
                    .alignment(Alignment::Center),
                area,
            );
            true
        }
        _ => false,
    }
}
