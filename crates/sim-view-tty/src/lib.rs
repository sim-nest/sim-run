//! Loadable terminal (CLI/TUI) view/edit surface for SIM.
//!
//! The thesis: a terminal is one *surface*, not a baked subcommand. The `sim`
//! binary stays a bootloader; this crate is a library loaded at runtime that
//! projects a [`Scene`](sim_lib_scene) to text and reduces terminal key input to
//! [`Intent`](sim_lib_intent) values. Nothing here parses argv or owns the
//! process. Both directions are pure and deterministic, so the whole surface is
//! testable without a tty:
//!
//! - [`render_scene`] fits a scene to a `SurfaceCaps` (via the view crate's
//!   density projection) and walks it to stable ASCII.
//! - [`intent_from_key`] turns a normalized [`KeyInput`] into a validated Intent.
//!
//! The CLI and TUI presets differ only in advertised capabilities -- a `cli`
//! surface is keyboard-only ANSI, a `tui` surface adds pointer input and a
//! richer palette -- which the projection ranker reads. Build them with
//! [`cli_caps`] and [`tui_caps`].
//!
//! # Example
//!
//! ```
//! use sim_view_tty::{cli_caps, render_scene};
//!
//! let scene = sim_lib_scene::build::text_node("ready");
//! let text = render_scene(&scene, &cli_caps("tty.local.1"));
//! assert_eq!(text, "ready");
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod caps;
mod entrypoint;
mod input;
mod render;

pub use caps::{cli_caps, tui_caps};
pub use entrypoint::{TtyViewLib, tty_intent_symbol, tty_render_symbol};
pub use input::{KeyInput, intent_from_key, palette_intent_from_colon};
pub use render::{render_palette, render_scene};

#[cfg(test)]
mod tests;
