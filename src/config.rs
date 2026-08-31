use crate::domain::League;
use crate::tiles::packer::LayoutPref;
use crate::tiles::ScoreStyle;
use std::fs;
use std::path::Path;
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
    pub layout: LayoutPref,
    pub favorites: Vec<Favorite>,
    /// Theme name: any loaded theme (`theme::names()` — the built-ins plus
    /// `<config_dir>/themes/*.toml`). Kept as a string so an unknown value
    /// degrades to broadcast (with a stderr note naming the valid set)
    /// instead of failing the whole config load.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Score digit rendering: "big" (sextant digits, default) | "compact"
    /// (single row). Replaces the old GAMEDAY_BIG_SCORES env hack.
    #[serde(default)]
    pub score_style: ScoreStyle,
}

fn default_theme() -> String {
    crate::theme::BUILTIN_NAMES[0].to_string()
}

impl Config {
    pub fn default_all() -> Self {
        Self {
            enabled_tabs: League::ALL.to_vec(),
            layout: LayoutPref::Auto,
            favorites: vec![],
            theme: default_theme(),
            score_style: ScoreStyle::default(),
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

pub fn prune_pins(pins: Vec<Pin>, now: OffsetDateTime) -> Vec<Pin> {
    pins.into_iter()
        .filter(|p| match p.final_at {
            Some(t) => now - t < Duration::hours(6),
            None => true,
        })
        .collect()
}
