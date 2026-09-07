//! `:` command grammar: one static registry table drives parsing, completion,
//! and error text, so the valid set the user sees is always the set that
//! parses. Pure — applying a `Cmd` to the app lives in `input.rs`.

use crate::domain::League;
use crate::rank::SortKey;
use crate::theme;

/// A parsed `:` command, ready to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    GoLeague(League),
    GoHome,
    Plays,
    Standings(Option<League>),
    ConfigView,
    /// `:theme <name>` applies directly (canonical loaded name); `:theme`
    /// alone opens the picker.
    Theme(Option<String>),
    /// `:sort` alone cycles WATCH → TIME → LEAGUE → WATCH; `:sort <key>`
    /// sets it directly.
    Sort(Option<SortKey>),
    /// `:tv` enters the TV view.
    Tv,
    Pin(String),
    Quit,
    /// `:help` opens the `?` overlay — U5: the overlay and the `:` grammar
    /// name the same feature, so both should open it.
    Help,
    /// `:notify test` sends one notification through whatever backend is
    /// installed, past the config switch and the per-kind gap.
    NotifyTest,
}

/// Argument shape a registry entry accepts; drives both parse and complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgSpec {
    None,
    OptLeague,
    Theme,
    Sort,
    Abbr,
    NotifyTest,
}

const SORT_VALUES: &[&str] = &["watch", "time", "league"];

/// Every command name the prompt accepts, in the order errors and completion
/// list them. League slugs lead; a test pins this list to `League::ALL` so a
/// new league can't ship without its jump command.
const REGISTRY: &[(&str, ArgSpec)] = &[
    ("nfl", ArgSpec::None),
    ("cfb", ArgSpec::None),
    ("cbb", ArgSpec::None),
    ("nba", ArgSpec::None),
    ("wnba", ArgSpec::None),
    ("nhl", ArgSpec::None),
    ("mlb", ArgSpec::None),
    ("epl", ArgSpec::None),
    ("mls", ArgSpec::None),
    ("home", ArgSpec::None),
    ("all", ArgSpec::None),
    ("plays", ArgSpec::None),
    ("standings", ArgSpec::OptLeague),
    ("config", ArgSpec::None),
    ("theme", ArgSpec::Theme),
    ("sort", ArgSpec::Sort),
    ("tv", ArgSpec::None),
    ("pin", ArgSpec::Abbr),
    ("notify", ArgSpec::NotifyTest),
    ("help", ArgSpec::None),
    ("q", ArgSpec::None),
    ("quit", ArgSpec::None),
];

fn valid_names() -> String {
    REGISTRY
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join("|")
}

fn league_slugs() -> String {
    League::ALL.map(League::slug).join("|")
}

fn theme_names() -> String {
    theme::names().join("|")
}

/// Candidate argument values for completion, per spec. Abbr is free text —
/// nothing to complete. Theme names are the *loaded* set (built-ins plus the
/// user's files), so completion and the error text always agree.
fn arg_values(spec: ArgSpec) -> Vec<String> {
    match spec {
        ArgSpec::None | ArgSpec::Abbr => vec![],
        ArgSpec::OptLeague => League::ALL.iter().map(|l| l.slug().to_string()).collect(),
        ArgSpec::Theme => theme::names(),
        ArgSpec::Sort => SORT_VALUES.iter().map(|s| s.to_string()).collect(),
        ArgSpec::NotifyTest => vec!["test".to_string()],
    }
}

pub fn parse(input: &str) -> Result<Cmd, String> {
    let mut parts = input.split_whitespace();
    let Some(raw_name) = parts.next() else {
        return Err(format!("empty command, valid: {}", valid_names()));
    };
    let name = raw_name.to_ascii_lowercase();
    let Some((name, spec)) = REGISTRY.iter().find(|(n, _)| *n == name) else {
        return Err(format!(
            "unknown command {raw_name:?}, valid: {}",
            valid_names()
        ));
    };
    let arg = parts.next();
    if let Some(extra) = parts.next() {
        return Err(format!("too many arguments: {name:?} got extra {extra:?}"));
    }
    match spec {
        ArgSpec::None => {
            if let Some(arg) = arg {
                return Err(format!("{name:?} takes no argument, got {arg:?}"));
            }
            Ok(match *name {
                "home" | "all" => Cmd::GoHome,
                "plays" => Cmd::Plays,
                "config" => Cmd::ConfigView,
                "tv" => Cmd::Tv,
                "help" => Cmd::Help,
                "q" | "quit" => Cmd::Quit,
                slug => Cmd::GoLeague(
                    League::from_slug(slug).expect("registry league entries match League::ALL"),
                ),
            })
        }
        ArgSpec::OptLeague => match arg {
            None => Ok(Cmd::Standings(None)),
            Some(a) => match League::from_slug(&a.to_ascii_lowercase()) {
                Some(l) => Ok(Cmd::Standings(Some(l))),
                None => Err(format!("unknown league {a:?}, valid: {}", league_slugs())),
            },
        },
        ArgSpec::Theme => match arg {
            None => Ok(Cmd::Theme(None)),
            Some(a) => match theme::lookup(a) {
                Some(entry) => Ok(Cmd::Theme(Some(entry.name))),
                None => Err(format!("unknown theme {a:?}, valid: {}", theme_names())),
            },
        },
        ArgSpec::Sort => match arg.map(str::to_ascii_lowercase).as_deref() {
            // Bare `:sort` cycles — no argument is not an error here.
            None => Ok(Cmd::Sort(None)),
            Some("watch") => Ok(Cmd::Sort(Some(SortKey::Watch))),
            Some("time") => Ok(Cmd::Sort(Some(SortKey::Time))),
            Some("league") => Ok(Cmd::Sort(Some(SortKey::League))),
            Some(other) => Err(format!(
                "unknown sort {other:?}, valid: {}",
                SORT_VALUES.join("|")
            )),
        },
        ArgSpec::Abbr => match arg {
            None => Err(format!("{name:?} needs a team abbr, e.g. :pin kc")),
            Some(a) => Ok(Cmd::Pin(a.to_string())),
        },
        ArgSpec::NotifyTest => match arg {
            Some("test") => Ok(Cmd::NotifyTest),
            None | Some(_) => Err(format!("{name:?} takes \"test\", got {arg:?}")),
        },
    }
}

/// Completions for a partial prompt line, as full replacement lines.
/// Before the first space: command names by prefix. After it: the argument
/// values the command's spec allows, by prefix ("theme " -> every theme).
pub fn complete(input: &str) -> Vec<String> {
    let lc = input.to_ascii_lowercase();
    match lc.split_once(' ') {
        None => REGISTRY
            .iter()
            .filter(|(name, _)| name.starts_with(&lc))
            .map(|(name, _)| name.to_string())
            .collect(),
        Some((raw_name, frag)) => {
            let frag = frag.trim_start();
            let Some((name, spec)) = REGISTRY.iter().find(|(n, _)| *n == raw_name) else {
                return vec![];
            };
            arg_values(*spec)
                .into_iter()
                .filter(|v| v.to_ascii_lowercase().starts_with(frag))
                .map(|v| format!("{name} {v}"))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_league_theme_pin_and_errors() {
        assert_eq!(parse("nfl").unwrap(), Cmd::GoLeague(League::Nfl));
        assert_eq!(
            parse("standings nba").unwrap(),
            Cmd::Standings(Some(League::Nba))
        );
        assert_eq!(parse("standings").unwrap(), Cmd::Standings(None));
        assert_eq!(
            parse("theme gruvbox").unwrap(),
            Cmd::Theme(Some("gruvbox".into()))
        );
        assert_eq!(
            parse("theme").unwrap(),
            Cmd::Theme(None),
            "bare :theme opens the picker"
        );
        assert_eq!(parse("pin kc").unwrap(), Cmd::Pin("kc".into()));
        let err = parse("foo").unwrap_err();
        assert!(
            err.contains("\"foo\"") && err.contains("nfl") && err.contains("standings"),
            "{err}"
        );
    }

    #[test]
    fn completes_prefixes() {
        assert!(complete("st").contains(&"standings".to_string()));
        assert!(complete("theme ").contains(&"theme gruvbox".to_string()));
    }

    #[test]
    fn theme_completion_lists_every_loaded_name() {
        let all = complete("theme ");
        assert_eq!(all.len(), theme::names().len(), "{all:?}");
        for name in theme::BUILTIN_NAMES {
            assert!(
                all.contains(&format!("theme {name}")),
                "missing {name}: {all:?}"
            );
        }
        assert_eq!(complete("theme gr"), vec!["theme gruvbox"]);
        assert_eq!(
            complete("theme STUD"),
            vec!["theme studio"],
            "case-insensitive"
        );
        // A user theme installed on this thread completes too.
        theme::install(theme::Entry {
            name: "zebra".into(),
            theme: theme::builtin("gruvbox"),
            user: true,
        });
        assert_eq!(complete("theme z"), vec!["theme zebra"]);
        assert_eq!(
            parse("theme Zebra").unwrap(),
            Cmd::Theme(Some("zebra".into()))
        );
    }

    #[test]
    fn every_league_slug_is_a_command() {
        for league in League::ALL {
            assert_eq!(
                parse(league.slug()).unwrap(),
                Cmd::GoLeague(league),
                "league {} missing from REGISTRY",
                league.slug()
            );
        }
    }

    #[test]
    fn parses_remaining_commands() {
        assert_eq!(parse("home").unwrap(), Cmd::GoHome);
        assert_eq!(parse("all").unwrap(), Cmd::GoHome);
        assert_eq!(parse("plays").unwrap(), Cmd::Plays);
        assert_eq!(parse("config").unwrap(), Cmd::ConfigView);
        assert_eq!(
            parse("sort league").unwrap(),
            Cmd::Sort(Some(SortKey::League))
        );
        assert_eq!(parse("tv").unwrap(), Cmd::Tv);
        assert_eq!(parse("q").unwrap(), Cmd::Quit);
        assert_eq!(parse("quit").unwrap(), Cmd::Quit);
        // Case-insensitive, whitespace-tolerant.
        assert_eq!(
            parse("  THEME Gruvbox ").unwrap(),
            Cmd::Theme(Some("gruvbox".into()))
        );
    }

    // `:sort`/`:tv` land; `:layout`/`:score` and the old tile-grammar keys
    // are removed from the registry.
    #[test]
    fn sort_and_tv_parse_and_layout_score_are_gone() {
        assert_eq!(parse("sort").unwrap(), Cmd::Sort(None));
        assert_eq!(parse("sort time").unwrap(), Cmd::Sort(Some(SortKey::Time)));
        assert!(parse("sort sideways")
            .unwrap_err()
            .contains("watch|time|league"));
        assert_eq!(parse("tv").unwrap(), Cmd::Tv);
        let err = parse("layout").unwrap_err();
        assert!(err.contains("unknown command"), "{err}");
        assert!(!valid_names().contains("score"), "registry cleaned");
    }

    #[test]
    fn argument_errors_name_the_value_and_the_valid_set() {
        let err = parse("theme solarized").unwrap_err();
        assert!(
            err.contains("\"solarized\"") && err.contains("broadcast|studio|gruvbox"),
            "{err}"
        );
        // The valid set is the whole loaded list, not just the built-ins: a
        // user theme installed on this thread has to appear in it too.
        theme::install(theme::Entry {
            name: "zebra".into(),
            theme: theme::builtin("gruvbox"),
            user: true,
        });
        let err = parse("theme solarized").unwrap_err();
        assert!(
            err.contains("zebra"),
            "the valid set is the whole loaded list: {err}"
        );
        let err = parse("standings xfl").unwrap_err();
        assert!(
            err.contains("\"xfl\"") && err.contains("nfl") && err.contains("mls"),
            "{err}"
        );
        let err = parse("sort sideways").unwrap_err();
        assert!(
            err.contains("\"sideways\"") && err.contains("watch|time|league"),
            "{err}"
        );
        let err = parse("pin").unwrap_err();
        assert!(err.contains("abbr"), "{err}");
        let err = parse("nfl extra").unwrap_err();
        assert!(err.contains("\"extra\""), "{err}");
        let err = parse("").unwrap_err();
        assert!(err.contains("nfl"), "{err}");
    }

    #[test]
    fn completion_covers_names_and_args() {
        assert_eq!(complete("nf"), vec!["nfl".to_string()]);
        let n: Vec<String> = complete("n");
        assert!(
            n.contains(&"nfl".into()) && n.contains(&"nba".into()) && n.contains(&"nhl".into())
        );
        assert_eq!(
            complete("standings n"),
            vec!["standings nfl", "standings nba", "standings nhl"]
        );
        assert_eq!(
            complete("sort "),
            vec!["sort watch", "sort time", "sort league"]
        );
        assert!(complete("pin k").is_empty(), "abbrs are free text");
        assert!(complete("bogus x").is_empty());
        assert_eq!(
            complete("").len(),
            REGISTRY.len(),
            "empty prompt offers everything"
        );
    }

    #[test]
    fn notify_test_parses_and_completes() {
        assert_eq!(parse("notify test").unwrap(), Cmd::NotifyTest);
        let err = parse("notify").unwrap_err();
        assert!(err.contains("test"), "{err}");
        let err = parse("notify foo").unwrap_err();
        assert!(err.contains("test"), "{err}");
        assert_eq!(complete("notify "), vec!["notify test"]);
    }
}
