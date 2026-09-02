use crate::domain::League;
use crate::rank::SortKey;
use std::fs;
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io {0}")]
    Io(#[from] std::io::Error),
    #[error("toml serialize {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("toml deserialize {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("json {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Favorite {
    pub league: League,
    pub team_abbr: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pin {
    pub game_id: String,
    pub league: League,
    pub final_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub enabled_tabs: Vec<League>,
    pub favorites: Vec<Favorite>,
    /// Theme name: any loaded theme (`theme::names()` — the built-ins plus
    /// `<config_dir>/themes/*.toml`). Kept as a string so an unknown value
    /// degrades to broadcast (with a stderr note naming the valid set)
    /// instead of failing the whole config load.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Board order: watch (watchability, default) | time | league. The board
    /// only re-sorts on a data event — see `rank::OrderState`.
    #[serde(default)]
    pub sort: SortKey,
}

fn default_theme() -> String {
    crate::theme::BUILTIN_NAMES[0].to_string()
}

impl Config {
    pub fn default_all() -> Self {
        Self {
            enabled_tabs: League::ALL.to_vec(),
            favorites: vec![],
            theme: default_theme(),
            sort: SortKey::default(),
        }
    }

    pub fn load_from(dir: &Path) -> Result<Self, ConfigError> {
        let path = dir.join("config.toml");
        if !path.exists() {
            return Ok(Self::default_all());
        }
        let text = fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save_to(&self, dir: &Path) -> Result<(), ConfigError> {
        fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self)?;
        fs::write(dir.join("config.toml"), text)?;
        Ok(())
    }
}

pub fn load_pins(dir: &Path) -> Result<Vec<Pin>, ConfigError> {
    let path = dir.join("pins.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn save_pins(dir: &Path, pins: &[Pin]) -> Result<(), ConfigError> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(pins)?;
    fs::write(dir.join("pins.json"), text)?;
    Ok(())
}

/// Where gameday reads and writes. `dir` is the only place writes ever land;
/// `legacy_read_from`, when set, is a pre-v3 directory reads come from until
/// the user moves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirResolution {
    pub dir: PathBuf,
    pub legacy_read_from: Option<PathBuf>,
}

/// `--config-dir` > `$XDG_CONFIG_HOME/gameday` > `~/.config/gameday`. When the
/// chosen dir has no config.toml but the pre-v3 platform location does, reads
/// come from there and `main` prints a note saying where writes now go. An
/// explicit `--config-dir` never consults the legacy location, and a legacy
/// path equal to the resolved dir (Linux, where they coincide) is not legacy.
pub fn resolve_dir(
    override_dir: Option<PathBuf>,
    home: &Path,
    xdg: Option<&Path>,
    legacy: Option<&Path>,
) -> DirResolution {
    if let Some(dir) = override_dir {
        return DirResolution {
            dir,
            legacy_read_from: None,
        };
    }
    let dir = xdg
        .map(|x| x.join("gameday"))
        .unwrap_or_else(|| home.join(".config").join("gameday"));
    let legacy_read_from = match legacy {
        Some(l)
            if l != dir && !dir.join("config.toml").exists() && l.join("config.toml").exists() =>
        {
            Some(l.to_path_buf())
        }
        _ => None,
    };
    DirResolution {
        dir,
        legacy_read_from,
    }
}

/// A load that never fails: a broken file yields defaults plus a human-readable
/// error naming the file, line, and the valid set. The caller holds the error
/// so it can refuse to overwrite what it could not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOutcome<T> {
    pub value: T,
    pub error: Option<String>,
}

pub fn load_config(r: &DirResolution) -> LoadOutcome<Config> {
    let read_dir = r.legacy_read_from.as_deref().unwrap_or(&r.dir);
    match Config::load_from(read_dir) {
        Ok(c) => LoadOutcome {
            value: c,
            error: None,
        },
        Err(e) => LoadOutcome {
            value: Config::default_all(),
            error: Some(describe_error(&read_dir.join("config.toml"), &e)),
        },
    }
}

pub fn load_pins_outcome(r: &DirResolution) -> LoadOutcome<Vec<Pin>> {
    let read_dir = r.legacy_read_from.as_deref().unwrap_or(&r.dir);
    match load_pins(read_dir) {
        Ok(p) => LoadOutcome {
            value: p,
            error: None,
        },
        Err(e) => LoadOutcome {
            value: vec![],
            error: Some(describe_error(&read_dir.join("pins.json"), &e)),
        },
    }
}

/// "config.toml:1: unknown variant `NFLL`, expected one of … — valid leagues:
/// nfl|cfb|…". The line comes from the TOML span; without one the file name
/// still leads.
pub fn describe_error(path: &Path, e: &ConfigError) -> String {
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config.toml");
    match e {
        ConfigError::TomlDe(te) => {
            let line = te.span().and_then(|s| {
                fs::read_to_string(path)
                    .ok()
                    .map(|t| t[..s.start.min(t.len())].matches('\n').count() + 1)
            });
            let msg = te.message();
            let hint = if msg.contains("League") || msg.contains("variant") {
                format!(
                    " — valid leagues: {}",
                    League::ALL
                        .iter()
                        .map(|l| l.slug())
                        .collect::<Vec<_>>()
                        .join("|")
                )
            } else {
                String::new()
            };
            match line {
                Some(l) => format!("{file}:{l}: {msg}{hint}"),
                None => format!("{file}: {msg}{hint}"),
            }
        }
        other => format!("{file}: {other}"),
    }
}

pub fn prune_pins(pins: Vec<Pin>, now: OffsetDateTime) -> Vec<Pin> {
    pins.into_iter()
        .filter(|p| match p.final_at {
            Some(t) => now - t < Duration::hours(6),
            None => true,
        })
        .collect()
}
