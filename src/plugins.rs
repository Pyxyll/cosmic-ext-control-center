//! Plugin discovery: scan well-known dirs for `*.ron` manifests and parse
//! them into `Manifest`s for the add-module palette + instantiation.

use crate::module::manifest::Manifest;
use std::fs;
use std::path::PathBuf;

/// Directories searched for plugin manifests, in priority order. User config
/// first, then system data dirs.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(format!(
            "{home}/.config/cosmic-ext-control-center/plugins"
        )));
    }
    let xdg = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for d in xdg.split(':').filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(d).join("cosmic-ext-control-center/plugins"));
    }
    dirs
}

/// Parse every `*.ron` manifest found across the plugin dirs. Malformed
/// manifests are logged and skipped, never fatal.
pub fn discover() -> Vec<Manifest> {
    let mut out = Vec::new();
    for dir in plugin_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ron") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(s) => match Manifest::parse(&s) {
                    Ok(m) => out.push(m),
                    Err(e) => eprintln!("ccc: skipping bad plugin {path:?}: {e}"),
                },
                Err(e) => eprintln!("ccc: cannot read {path:?}: {e}"),
            }
        }
    }
    out
}
