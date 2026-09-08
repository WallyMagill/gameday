//! gameday is a binary. This library exists so the integration tests,
//! `gameday frame` and `gameday dump` can reach the same code; it is not a
//! stable API and carries no semver promise. What 1.0.0 promises: the CLI
//! flags, `config.toml`'s keys, `pins.json`, and the `--once --json` schema.

pub mod alerts;
pub mod app;
pub mod board;
pub mod command;
pub mod config;
pub mod demo;
pub mod domain;
pub mod dump;
pub mod filter;
pub mod frame;
pub mod home;
pub mod input;
pub mod keymap;
pub mod log;
pub mod notify;
pub mod once;
pub mod poll;
pub mod provider;
pub mod rank;
pub mod sim;
pub mod text;
pub mod theme;
pub mod ticker;
pub mod tiles;
pub mod views;

pub use domain::*;
