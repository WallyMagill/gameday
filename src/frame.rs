//! `gameday frame`: render exactly ONE surface, offscreen, through the real
//! renderer — the parameterized sibling of `gameday dump`.
//!
//!   gameday frame --view tv --theme studio --size 80x24 \
//!                 --scenario redzone --tick 40 --out out/design/tv-studio-80.png
//!
//! `dump` is the fixed gallery: 23 pinned stems other tasks verify against.
//! `frame` is the design loop's one-shot: any view, any theme (including a
//! candidate theme file that is not a built-in), any size, any scripted sim
//! state, written where the caller asks. Both go through the same setup
//! functions ([`crate::dump::setup`]), the same demo app, the same
//! HTML/ANSI serializers and the same batched-Chrome screenshot pipeline —
//! a frame is a `dump` gallery of one page.
//!
//! Every knob is deterministic: the sim tick is a pure function of N, the
//! clock is frozen, and no file name carries a timestamp. Two runs of the
//! same command produce the same bytes.

use crate::app::{App, Tab};
use crate::config::{Config, Favorite};
use crate::demo;
use crate::domain::{League, Status};
use crate::dump::{self, setup, Page};
use crate::provider::map::map_scoreboard;
use crate::theme;
use std::path::{Path, PathBuf};
use time::macros::datetime;

/// A surface `--view` can name. The setup each one runs is the *same*
/// function the matching `dump` stem uses, so a frame and its gallery
/// counterpart can never drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameView {
    Board,
    Tv,
    Zoom,
    CutFull,
    CutBand,
    Standings,
    Plays,
    Config,
    Help,
    ThemePicker,
    Filter,
}

impl FrameView {
    /// CLI spelling -> view, in help order. The first entry is the default.
    pub const ALL: [(&'static str, FrameView); 11] = [
        ("board", FrameView::Board),
        ("tv", FrameView::Tv),
        ("zoom", FrameView::Zoom),
        ("cut-full", FrameView::CutFull),
        ("cut-band", FrameView::CutBand),
        ("standings", FrameView::Standings),
        ("plays", FrameView::Plays),
        ("config", FrameView::Config),
        ("help", FrameView::Help),
        ("theme-picker", FrameView::ThemePicker),
        ("filter", FrameView::Filter),
    ];

    pub fn parse(s: &str) -> Result<FrameView, String> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == s)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("unknown view {s:?}, valid: {}", Self::valid()))
    }

    pub fn valid() -> String {
        Self::ALL.map(|(n, _)| n).join("|")
    }

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, v)| *v == self)
            .map(|(n, _)| *n)
            .expect("every FrameView is in ALL")
    }

    fn setup(self) -> fn(&mut App) -> Result<(), String> {
        match self {
            FrameView::Board => setup::home,
            FrameView::Tv => setup::tv,
            FrameView::Zoom => setup::zoom,
            FrameView::CutFull => setup::cut_full,
            FrameView::CutBand => setup::cut_band,
            FrameView::Standings => setup::standings,
            FrameView::Plays => setup::plays_feed,
            FrameView::Config => setup::config,
            FrameView::Help => setup::help,
            FrameView::ThemePicker => setup::theme_picker,
            FrameView::Filter => setup::filter,
        }
    }
}

/// A named sim state. These NAME states the scripted demo already produces —
/// a scenario either pins the tick the sim reaches that state at, or filters
/// the seeded slate down. None of them invents game machinery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// The whole demo slate: every league, every section. Tick 0.
    FullSlate,
    /// KC at the goal line, one tick before the scripted touchdown — the
    /// hero wearing RED ZONE with a live drive under it.
    Redzone,
    /// One league, one row per section: the quiet Tuesday board.
    ThinSlate,
    /// Nothing live and nothing scheduled — only the FINAL section.
    FinalsOnly,
    /// No games at all: the empty board's own message.
    Empty,
    /// The one scripted beat that re-sorts the live list (`sim::NUDGE_TICK`):
    /// the bases load and the rows below carry `↑n`.
    NudgeResort,
    /// The 2026-09-05 CFB afternoon that exposed R1 — 68 events, 18 live,
    /// real win probabilities; read from
    /// `fixtures/review-slate-2026-09-05.json` at run time (it is a
    /// megabyte; dev only), mapped through `provider::map::map_scoreboard`.
    ReviewSlate,
}

impl Scenario {
    pub const ALL: [(&'static str, Scenario); 7] = [
        ("full-slate", Scenario::FullSlate),
        ("redzone", Scenario::Redzone),
        ("thin-slate", Scenario::ThinSlate),
        ("finals-only", Scenario::FinalsOnly),
        ("empty", Scenario::Empty),
        ("nudge-resort", Scenario::NudgeResort),
        ("review-slate", Scenario::ReviewSlate),
    ];

    pub fn parse(s: &str) -> Result<Scenario, String> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == s)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("unknown scenario {s:?}, valid: {}", Self::valid()))
    }

    pub fn valid() -> String {
        Self::ALL.map(|(n, _)| n).join("|")
    }

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, v)| *v == self)
            .map(|(n, _)| *n)
            .expect("every Scenario is in ALL")
    }

    /// The tick this scenario is *about*, used when `--tick` is not given.
    /// Two scenarios are pinned beats of the script and the rest read the
    /// seeded board, so their tick is 0.
    pub fn default_tick(self) -> u64 {
        match self {
            // One tick before the TD: the drive is still 2nd & Goal, so the
            // hero shows RED ZONE with a live fragment under it. At
            // KC_TD_TICK itself the situation is cleared by the score.
            Scenario::Redzone => crate::sim::KC_TD_TICK - 1,
            Scenario::NudgeResort => crate::sim::NUDGE_TICK,
            _ => 0,
        }
    }

    /// Shape the seeded boards. Filtering happens after the sim has run, so
    /// a filtered scenario at a late tick still shows that tick's scores.
    /// Every scenario but `ReviewSlate` always succeeds; that one reads a
    /// fixture off disk and names the failure rather than panicking.
    pub fn apply(self, app: &mut App) -> Result<(), String> {
        match self {
            Scenario::FullSlate | Scenario::Redzone | Scenario::NudgeResort => {}
            Scenario::ThinSlate => {
                // One league (the NFL board is the one with a game in every
                // status) and one game per status: three rows, three
                // sections — the board with nothing to rank.
                app.boards
                    .retain(|league, _| *league == crate::domain::League::Nfl);
                for games in app.boards.values_mut() {
                    let mut kept: Vec<crate::domain::Game> = Vec::new();
                    for status in [Status::Live, Status::Final, Status::Pre] {
                        if let Some(g) = games.iter().find(|g| g.status == status) {
                            kept.push(g.clone());
                        }
                    }
                    *games = kept;
                }
            }
            Scenario::FinalsOnly => {
                for games in app.boards.values_mut() {
                    games.retain(|g| g.status == Status::Final);
                }
            }
            Scenario::Empty => app.boards.clear(),
            Scenario::ReviewSlate => {
                // The demo's scripted order state and rank fingerprints must
                // not leak into the mapped slate — `render` builds this
                // scenario's app fresh (not through `dump::demo_app`), so
                // this only has to load the real capture onto it.
                // favorites/enabled_tabs for this scenario are set once, in
                // `render`'s fresh `Config` — not duplicated here.
                app.boards.clear();
                app.pins.clear();
                app.now_override = Some(datetime!(2026-09-05 16:52 -4));
                let body = std::fs::read_to_string(REVIEW_SLATE_FIXTURE).map_err(|e| {
                    format!(
                        "review-slate: {REVIEW_SLATE_FIXTURE} not found — run from the repo root ({e})"
                    )
                })?;
                let offset = time::UtcOffset::from_hms(-4, 0, 0).expect("-04:00 is a valid offset");
                let games = map_scoreboard(League::Cfb, &body, offset).map_err(|e| {
                    format!("review-slate: {REVIEW_SLATE_FIXTURE} failed to map: {e}")
                })?;
                app.apply_boards(League::Cfb, games, false);
                app.tab = Tab::Home;
            }
        }
        Ok(())
    }
}

/// The captured 2026-09-05 CFB scoreboard `Scenario::ReviewSlate` reads —
/// dev only, a megabyte on disk, never `include_str!`ed into the binary.
const REVIEW_SLATE_FIXTURE: &str = "fixtures/review-slate-2026-09-05.json";

/// Everything one `gameday frame` invocation renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub view: FrameView,
    pub scenario: Scenario,
    /// A built-in name, an installed user theme, or a path to a theme TOML.
    pub theme: String,
    pub cols: u16,
    pub rows: u16,
    /// `--tick N` when given; otherwise the scenario's own tick.
    pub tick: Option<u64>,
    /// The PNG the caller asked for. The `.ansi` and `.html` beside it carry
    /// the same stem.
    pub out: PathBuf,
}

/// The size a design frame is rendered at when `--size` is not given — the
/// same 120x36 the gallery uses ([`dump::DUMP_COLS`]/[`dump::DUMP_ROWS`]), so
/// an unflagged frame is directly comparable to its gallery counterpart.
pub const DEFAULT_SIZE: (u16, u16) = (dump::DUMP_COLS, dump::DUMP_ROWS);

/// The floor the app's own layout is built to. Below this the views stop
/// having room for their sections and the frame would be a picture of a
/// degraded layout rather than of a design; the app's minimum-size message
/// names the same numbers (the `src/app` size ladder).
pub const MIN_SIZE: (u16, u16) = (40, 12);
/// The ceiling. 400x200 is far past any real terminal and past the 22-page
/// gallery's largest capture; it exists so a typo (`--size 8000x2400`) is an
/// error naming the limit instead of a multi-gigabyte HTML file.
pub const MAX_SIZE: (u16, u16) = (400, 200);

/// `WxH` -> (cols, rows). Errors name the value, the two numbers, and the
/// range — a size is the flag people fat-finger most.
pub fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("--size expects WxH (e.g. 120x36), got {s:?}"))?;
    let cols: u16 = w
        .trim()
        .parse()
        .map_err(|_| format!("--size width must be a number, got {w:?} in {s:?}"))?;
    let rows: u16 = h
        .trim()
        .parse()
        .map_err(|_| format!("--size height must be a number, got {h:?} in {s:?}"))?;
    if cols < MIN_SIZE.0 || rows < MIN_SIZE.1 || cols > MAX_SIZE.0 || rows > MAX_SIZE.1 {
        return Err(format!(
            "--size {cols}x{rows} is out of range, valid: {}x{} to {}x{}",
            MIN_SIZE.0, MIN_SIZE.1, MAX_SIZE.0, MAX_SIZE.1
        ));
    }
    Ok((cols, rows))
}

/// Resolve `--theme`: a loaded theme by name, or a theme TOML installed for
/// this process only. Installing rather than extending `BUILTIN_NAMES` is
/// the point — a candidate palette renders without shipping in the picker.
/// Returns the canonical theme name.
pub fn resolve_theme(spec: &str) -> Result<String, String> {
    let looks_like_path = spec.ends_with(".toml") || spec.contains(std::path::MAIN_SEPARATOR);
    if !looks_like_path {
        return theme::lookup(spec).map(|e| e.name).ok_or_else(|| {
            format!(
                "unknown theme {spec:?}, valid: {} (or a path to a theme .toml)",
                theme::names().join("|")
            )
        });
    }
    let path = Path::new(spec);
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("theme file {} could not be read: {e}", path.display()))?;
    let (name, th) = theme::parse_theme(&text)
        .map_err(|e| format!("theme file {} did not parse: {e}", path.display()))?;
    theme::install(theme::Entry {
        name: name.clone(),
        theme: th,
        user: true,
    });
    Ok(name)
}

/// Render one frame's buffer: the demo app (or, for `ReviewSlate`, a fresh
/// app built straight from the mapped fixture — the demo's scripted order
/// state and rank fingerprints must not leak into it) at the scenario's
/// state, drawn through the real view setup and the real renderer. No
/// Chrome, no files — this is what `run` writes to disk and what a test
/// inspects directly.
pub(crate) fn render(spec: &Spec) -> std::io::Result<ratatui::buffer::Buffer> {
    let theme_name = resolve_theme(&spec.theme).map_err(std::io::Error::other)?;
    let tick = spec.tick.unwrap_or_else(|| spec.scenario.default_tick());
    dump::with_theme(&theme_name, || {
        let dir = std::env::temp_dir().join(format!("gameday-frame-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut app = match spec.scenario {
            Scenario::ReviewSlate => {
                let config = Config {
                    enabled_tabs: vec![League::Cfb],
                    favorites: vec![Favorite {
                        league: League::Cfb,
                        team_abbr: "ORE".into(),
                    }],
                    ..demo::demo_config()
                };
                let offset = time::UtcOffset::from_hms(-4, 0, 0).expect("-04:00 is a valid offset");
                App::new(config, vec![], dir, offset)
            }
            _ => dump::demo_app(dir, tick),
        };
        spec.scenario
            .apply(&mut app)
            .map_err(std::io::Error::other)?;
        (spec.view.setup())(&mut app).map_err(|e| {
            std::io::Error::other(format!(
                "view {} cannot render scenario {}: {e}",
                spec.view.name(),
                spec.scenario.name()
            ))
        })?;
        // TestBackend's Error is Infallible, so this unwrap cannot fire.
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(spec.cols, spec.rows))
                .unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        Ok::<_, std::io::Error>(term.backend().buffer().clone())
    })
}

/// Render one frame and write `<stem>.html`, `<stem>.ansi` and (when Chrome
/// is available) `<stem>.png` beside the `--out` path. The directory is
/// created. Returns the stem's directory and stem, for the caller's report.
pub fn run(spec: &Spec) -> std::io::Result<()> {
    let out_dir = spec.out.parent().unwrap_or(Path::new(".")).to_path_buf();
    let stem = spec
        .out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "--out {} has no file name (expected a path like out/design/tv.png)",
                spec.out.display()
            ))
        })?;
    std::fs::create_dir_all(&out_dir)?;
    let theme_name = resolve_theme(&spec.theme).map_err(std::io::Error::other)?;
    let buf = render(spec)?;
    let page = Page {
        stem,
        cols: spec.cols,
        rows: spec.rows,
        theme: theme_name,
        buf,
    };
    let pages = [page];
    dump::write_pages(&out_dir, &pages)?;
    let chrome = dump::screenshot_pages(&out_dir, &pages);
    dump::verify_pages(&out_dir, &pages, chrome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(buf: &ratatui::buffer::Buffer) -> String {
        let area = *buf.area();
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn spec(view: &str, scenario: Scenario, out: PathBuf) -> Spec {
        Spec {
            view: FrameView::parse(view).unwrap(),
            scenario,
            theme: "broadcast".into(),
            cols: DEFAULT_SIZE.0,
            rows: DEFAULT_SIZE.1,
            tick: None,
            out,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gameday-frame-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bad_view_names_the_value_and_every_valid_view() {
        let err = FrameView::parse("jumbotron").unwrap_err();
        assert!(err.contains("jumbotron"), "{err}");
        for (name, _) in FrameView::ALL {
            assert!(err.contains(name), "error must list {name}: {err}");
        }
        assert_eq!(FrameView::parse("board").unwrap(), FrameView::Board);
        assert_eq!(
            FrameView::parse("theme-picker").unwrap(),
            FrameView::ThemePicker
        );
    }

    #[test]
    fn bad_scenario_names_the_value_and_every_valid_scenario() {
        let err = Scenario::parse("blowout").unwrap_err();
        assert!(err.contains("blowout"), "{err}");
        for (name, _) in Scenario::ALL {
            assert!(err.contains(name), "error must list {name}: {err}");
        }
        assert_eq!(
            Scenario::parse("nudge-resort").unwrap(),
            Scenario::NudgeResort
        );
    }

    #[test]
    fn bad_size_names_the_value_and_the_range() {
        assert_eq!(parse_size("80x24").unwrap(), (80, 24));
        assert_eq!(parse_size("120X36").unwrap(), (120, 36));
        let err = parse_size("80").unwrap_err();
        assert!(err.contains("80") && err.contains("WxH"), "{err}");
        let err = parse_size("axb").unwrap_err();
        assert!(err.contains("\"a\""), "the offending half is named: {err}");
        let err = parse_size("20x8").unwrap_err();
        assert!(err.contains("20x8") && err.contains("40x12"), "{err}");
        let err = parse_size("8000x2400").unwrap_err();
        assert!(err.contains("400x200"), "the ceiling is named: {err}");
    }

    #[test]
    fn unknown_theme_names_the_value_the_loaded_set_and_the_file_escape_hatch() {
        let err = resolve_theme("neon").unwrap_err();
        assert!(err.contains("neon"), "{err}");
        for name in theme::BUILTIN_NAMES {
            assert!(err.contains(name), "error must list {name}: {err}");
        }
        assert!(
            err.contains(".toml"),
            "the file escape hatch is named: {err}"
        );
        assert_eq!(resolve_theme("studio").unwrap(), "studio");
        // A missing file names the path, not just "not found".
        let err = resolve_theme("themes/nope.toml").unwrap_err();
        assert!(err.contains("themes/nope.toml"), "{err}");
    }

    /// The point of `--theme <path>`: a palette that is NOT a built-in
    /// renders, and renders in its own colors, without touching the picker's
    /// name list.
    #[test]
    fn a_theme_file_renders_without_joining_the_builtins() {
        let dir = scratch("theme-file");
        let path = dir.join("candidate.toml");
        // daygame was promoted into BUILTIN_NAMES at the render gate;
        // gruvbox-warm is the remaining gate candidate, still not a built-in.
        let cand = theme::candidate("gruvbox-warm");
        std::fs::write(&path, theme::to_toml(&cand.name, &cand.theme)).unwrap();
        let mut s = spec("board", Scenario::FullSlate, dir.join("x.png"));
        s.theme = path.display().to_string();
        let name = resolve_theme(&s.theme).unwrap();
        assert_eq!(name, "gruvbox-warm");
        assert!(
            !theme::BUILTIN_NAMES.contains(&"gruvbox-warm"),
            "still not a built-in"
        );
        let buf = dump::with_theme(&name, || {
            let d = std::env::temp_dir().join(format!("gameday-frame-c-{}", std::process::id()));
            std::fs::create_dir_all(&d).unwrap();
            let mut app = dump::demo_app(d, 0);
            let mut term =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            term.backend().buffer().clone()
        });
        assert_eq!(buf[(0, 0)].bg, cand.theme.bg, "the candidate's own ground");
        theme::uninstall("gruvbox-warm");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Each of the six scenarios must actually SHOW the state it is named
    /// for — a name with nothing behind it is worse than no scenario.
    #[test]
    fn every_scenario_shows_the_state_it_is_named_for() {
        let full = text_of(&render(&spec("board", Scenario::FullSlate, "x.png".into())).unwrap());
        assert!(full.contains("IN PLAY") && full.contains("FINAL"), "{full}");

        let red = text_of(&render(&spec("board", Scenario::Redzone, "x.png".into())).unwrap());
        assert!(red.contains("RED ZONE"), "the hero's red-zone chip:\n{red}");
        assert!(red.contains("Goal"), "a live goal-line drive:\n{red}");

        let thin = text_of(&render(&spec("board", Scenario::ThinSlate, "x.png".into())).unwrap());
        assert!(thin.contains("KC"), "the one live game survives:\n{thin}");
        // One league only: the NBA/NHL/MLB rows are gone.
        assert!(
            !thin.contains("EDM") && !thin.contains("NYY"),
            "one league only:\n{thin}"
        );

        let finals =
            text_of(&render(&spec("board", Scenario::FinalsOnly, "x.png".into())).unwrap());
        assert!(finals.contains("FINAL"), "the FINAL section:\n{finals}");
        assert!(!finals.contains("IN PLAY"), "nothing is live:\n{finals}");

        let empty = text_of(&render(&spec("board", Scenario::Empty, "x.png".into())).unwrap());
        assert!(
            !empty.contains("IN PLAY"),
            "no sections on an empty board:\n{empty}"
        );
        assert!(
            empty.contains("GAMEDAY"),
            "the chrome is still drawn:\n{empty}"
        );

        // The nudge scenario lands on the scripted re-sort: the cause is on
        // screen and the risen row wears its arrow.
        let nudge =
            text_of(&render(&spec("board", Scenario::NudgeResort, "x.png".into())).unwrap());
        assert!(nudge.contains("BASES LOADED"), "the cause:\n{nudge}");
        assert!(
            nudge.lines().any(|l| l.chars().nth(2) == Some('↑')),
            "the ↑n gutter:\n{nudge}"
        );
    }

    /// `--tick` beats the scenario's own tick (the scenario picks the
    /// interesting beat; an explicit flag is the caller overruling it).
    #[test]
    fn an_explicit_tick_overrides_the_scenarios_default() {
        assert_eq!(Scenario::Redzone.default_tick(), crate::sim::KC_TD_TICK - 1);
        assert_eq!(Scenario::NudgeResort.default_tick(), crate::sim::NUDGE_TICK);
        assert_eq!(Scenario::FullSlate.default_tick(), 0);
        let mut s = spec("board", Scenario::FullSlate, "x.png".into());
        s.tick = Some(crate::sim::KC_TD_TICK);
        let text = text_of(&render(&s).unwrap());
        assert!(text.contains("TOUCHDOWN"), "tick 15 is the TD:\n{text}");
    }

    /// A view whose subject the scenario removed must say so by name, not
    /// panic and not render a blank frame.
    #[test]
    fn a_view_that_needs_a_game_the_scenario_removed_names_both() {
        let dir = scratch("cut-empty");
        let mut s = spec("cut-full", Scenario::Empty, dir.join("cut.png"));
        s.theme = "broadcast".into();
        let err = run(&s).unwrap_err().to_string();
        assert!(err.contains("cut-full"), "the view is named: {err}");
        assert!(err.contains("empty"), "the scenario is named: {err}");
        assert!(err.contains("nfl-live"), "the missing game is named: {err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The write leg: every view renders to files at the size asked for, and
    /// the ANSI carries exactly that many rows and columns.
    #[test]
    fn each_view_writes_a_nonempty_frame_at_the_size_asked_for() {
        let dir = scratch("write");
        for (view, size) in [
            ("board", (80u16, 24u16)),
            ("tv", (120, 36)),
            ("zoom", (100, 30)),
            ("help", (120, 36)),
        ] {
            let mut s = spec(view, Scenario::FullSlate, dir.join(format!("{view}.png")));
            (s.cols, s.rows) = size;
            // Chrome is not required for the files this test judges.
            let theme_name = resolve_theme(&s.theme).unwrap();
            let buf = render(&s).unwrap();
            let pages = [Page {
                stem: view.to_string(),
                cols: s.cols,
                rows: s.rows,
                theme: theme_name,
                buf,
            }];
            dump::write_pages(&dir, &pages).unwrap();
            dump::verify_pages(&dir, &pages, false).unwrap();
            let ansi = std::fs::read_to_string(dir.join(format!("{view}.ansi"))).unwrap();
            assert_eq!(
                ansi.lines().count(),
                usize::from(size.1),
                "{view}: the ansi must carry {} rows",
                size.1
            );
            let widest = ansi
                .lines()
                .map(|l| {
                    // Strip SGR sequences; what is left is the row's cells.
                    let mut n = 0usize;
                    let mut chars = l.chars();
                    while let Some(c) = chars.next() {
                        if c == '\u{1b}' {
                            for c in chars.by_ref() {
                                if c == 'm' {
                                    break;
                                }
                            }
                        } else {
                            n += 1;
                        }
                    }
                    n
                })
                .max()
                .unwrap_or(0);
            assert_eq!(widest, usize::from(size.0), "{view}: the ansi row width");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_review_slate_scenario_is_the_saturday_board_with_a_favorite() {
        let spec = Spec {
            view: FrameView::Board,
            scenario: Scenario::ReviewSlate,
            theme: "broadcast".into(),
            cols: 120,
            rows: 40,
            tick: None,
            out: PathBuf::from("unused.png"),
        };
        let s = text_of(&render(&spec).unwrap());
        assert!(
            s.contains("BOIS") && s.contains("ORE"),
            "Boise at Oregon is on the board:\n{s}"
        );
        assert!(
            s.contains("MY GAMES") && s.contains("★"),
            "the scenario's favorite (ORE) sits in the band:\n{s}"
        );
        assert!(
            s.contains("SAT SEP 5") || s.contains("SEP 5"),
            "the clock is the capture's afternoon:\n{s}"
        );
        // League tags print only on a mixed board, so "no NFL tag" would be
        // vacuous here. The footer's count is the tooth: the fixture holds
        // exactly 68 events, so a demo game leaking in would read 69.
        assert!(
            s.contains("GAME 1/68"),
            "the board is the fixture's 68 games and nothing else\n{s}"
        );
    }
}
