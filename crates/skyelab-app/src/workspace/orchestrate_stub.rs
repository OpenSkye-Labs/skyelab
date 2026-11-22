//! Orchestration needs unix sockets; here threads run without SkyeLab's tools.
// shortcut: no named-pipe transport on Windows yet; add one when Windows users need delegation.

use gpui::App;
use skyelab_core::AccountsConfig;
use std::path::Path;

pub enum Grant {}

impl Grant {
    pub fn config_path(&self) -> &Path {
        match *self {}
    }
}

#[derive(Clone)]
pub struct Tools;

impl Tools {
    pub fn get(_: &App) -> Option<Tools> {
        None
    }

    pub fn grant(&self, _: i64, _: &str) -> Option<Grant> {
        None
    }
}

pub fn set_config(_: &App, _: &AccountsConfig) {}
