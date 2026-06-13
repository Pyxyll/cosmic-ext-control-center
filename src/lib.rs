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
