//! cosmic-ext-control-center: a modular, pluggable control center for COSMIC.
//!
//! The hub stores a list of `Module` instances (built-in or, later, plugin)
//! and renders them as draggable tiles in a reflowing grid. See `module` for
//! the abstraction and `app` for the shell.

pub mod app;
pub mod applet;
pub mod config;
pub mod module;
pub mod plugins;
pub mod theme;
pub mod widgets;

/// Prefer the Vulkan wgpu backend when a Vulkan driver is present, so the popup
/// renders on the GPU instead of silently falling back to llvmpipe (CPU software
/// rendering), which made it janky. Respects an explicit `WGPU_BACKEND`, and on
/// systems with no Vulkan ICD at all it leaves wgpu's default alone (so they
/// don't end up with no adapter). Call at the very start of `main`, before the
/// renderer initializes.
pub fn prefer_vulkan_backend() {
    if std::env::var_os("WGPU_BACKEND").is_some() {
        return; // honour an explicit override
    }
    if vulkan_icd_present() {
        // SAFETY: called at the top of main(), before any threads are spawned.
        unsafe { std::env::set_var("WGPU_BACKEND", "vulkan") };
    }
}

/// Whether the Vulkan loader would find at least one ICD (driver). Checks the
/// explicit override env vars and the loader's standard search directories.
fn vulkan_icd_present() -> bool {
    use std::path::{Path, PathBuf};
    if std::env::var_os("VK_ICD_FILENAMES").is_some()
        || std::env::var_os("VK_DRIVER_FILES").is_some()
    {
        return true;
    }
    let mut dirs = vec![
        PathBuf::from("/usr/share/vulkan/icd.d"),
        PathBuf::from("/etc/vulkan/icd.d"),
    ];
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(Path::new(&h).join("vulkan/icd.d"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(Path::new(&h).join(".local/share/vulkan/icd.d"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&xdg).map(|d| d.join("vulkan/icd.d")));
    }
    dirs.iter().any(|d| {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "json"))
            })
            .unwrap_or(false)
    })
}
