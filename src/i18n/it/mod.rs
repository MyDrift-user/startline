//! Italian text, keyed by the English in the code. One file per part of the app; see `crate::i18n`.

mod menu;
mod search;
mod settings;
mod quick;
mod policy;
mod system;

pub const ALL: &[&[(&str, &str)]] = &[menu::T, search::T, settings::T, quick::T, policy::T, system::T];
