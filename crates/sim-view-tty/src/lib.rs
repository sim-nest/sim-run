//! Loadable terminal (CLI/TUI) view/edit surface for SIM.
//!
//! The thesis: a terminal is one *surface*, not a baked subcommand. The `sim`
//! binary stays a bootloader; this crate is a library loaded at runtime that
//! projects a [`Scene`](sim_lib_scene) to text and reduces terminal key input to
//! [`Intent`](sim_lib_intent) values. Nothing here parses argv or owns the
//! process. Both directions are pure and deterministic, so the whole surface is
//! testable without a tty:
//!
//! - [`render_scene`] fits a scene to a [`SurfaceCaps`] (via the view crate's
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

mod input;
mod render;

pub use input::{KeyInput, intent_from_key, palette_intent_from_colon};
pub use render::render_scene;

use sim_lib_view::SurfaceCaps;
use sim_lib_view::palette::{Command, palette_scene};

/// Renders the shared command palette overlay to deterministic terminal text.
///
/// Builds the surface-neutral [`palette_scene`] for `commands` filtered by
/// `filter`, then walks it through the same
/// [`render_scene`] path used for every other scene, so the palette is just
/// another overlay on the terminal. Output is ASCII and deterministic: equal
/// inputs yield an equal `String`.
pub fn render_palette(commands: &[Command], filter: &str) -> String {
    let scene = palette_scene(commands, filter);
    render_scene(&scene, &cli_caps("tty.palette"))
}

/// Builds the `cli` surface capabilities with `client_id` set.
///
/// A `cli` surface is the baseline keyboard-only ANSI terminal. Panics only if
/// the built-in `cli` preset is missing, which is a build-time invariant of
/// [`sim_lib_view::surface`].
pub fn cli_caps(client_id: &str) -> SurfaceCaps {
    SurfaceCaps::from_preset("cli", client_id).expect("cli is a built-in surface preset")
}

/// Builds the `tui` surface capabilities with `client_id` set.
///
/// A `tui` surface extends the `cli` baseline with pointer input and a richer
/// (ansi256) palette. Panics only if the built-in `tui` preset is missing.
pub fn tui_caps(client_id: &str) -> SurfaceCaps {
    SurfaceCaps::from_preset("tui", client_id).expect("tui is a built-in surface preset")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::Expr;
    use sim_lib_intent::{intent_kind_of, validate_intent};
    use sim_lib_scene::build::{stack, text_node};
    use sim_lib_scene::node;
    use sim_value::build::{list, sym, text};

    /// A composed scene: a column stacking a heading, a two-row table, and a
    /// button.
    fn composed_scene() -> Expr {
        let table = node(
            "table",
            vec![
                ("columns", list(vec![text("name"), text("kind")])),
                (
                    "rows",
                    list(vec![
                        list(vec![text("alpha"), sym("scene/text")]),
                        list(vec![text("beta"), sym("scene/button")]),
                    ]),
                ),
            ],
        );
        let button = node("button", vec![("label", text("Run"))]);
        stack("column", vec![text_node("Surfaces"), table, button])
    }

    #[test]
    fn renders_composed_scene_to_exact_text() {
        let caps = cli_caps("tty.local.1");
        let text = render_scene(&composed_scene(), &caps);
        let expected = [
            "Surfaces",
            "name | kind",
            "alpha | scene/text",
            "beta | scene/button",
            "[Run]",
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[test]
    fn renders_field_and_badge_spellings() {
        let caps = cli_caps("tty.local.1");
        let field = node(
            "field",
            vec![("label", text("name")), ("value", text("alpha"))],
        );
        assert_eq!(render_scene(&field, &caps), "name: alpha");
        let badge = node(
            "badge",
            vec![("status", sym("ok")), ("label", text("done"))],
        );
        assert_eq!(render_scene(&badge, &caps), "<ok: done>");
    }

    #[test]
    fn unknown_kind_degrades_to_marker() {
        let caps = cli_caps("tty.local.1");
        // `graph` is a known baseline kind this surface does not specialize.
        let graph = node("graph", vec![("nodes", list(Vec::new()))]);
        assert_eq!(render_scene(&graph, &caps), "[graph]");
    }

    fn intent_kind_name(intent: &Expr) -> String {
        intent_kind_of(intent)
            .expect("intent is kind-tagged")
            .name
            .to_string()
    }

    fn assert_valid(key: &KeyInput, expected_kind: &str) -> Expr {
        let intent =
            intent_from_key(key, "main", "node-1", "value", 7).expect("key maps to an intent");
        validate_intent(&intent).expect("produced intent validates");
        assert_eq!(intent_kind_name(&intent), expected_kind);
        intent
    }

    #[test]
    fn enter_maps_to_invoke() {
        assert_valid(&KeyInput::Enter, "invoke");
    }

    #[test]
    fn arrows_map_to_select_and_move() {
        assert_valid(&KeyInput::Up, "select");
        assert_valid(&KeyInput::Down, "select");
        assert_valid(&KeyInput::Left, "move");
        assert_valid(&KeyInput::Right, "move");
    }

    #[test]
    fn char_maps_to_edit_field() {
        assert_valid(&KeyInput::Char('x'), "edit-field");
    }

    #[test]
    fn char_edit_targets_focused_field_not_root() {
        let intent = intent_from_key(&KeyInput::Char('x'), "main", "node-1", "title", 7)
            .expect("char with a focused field maps to an intent");
        let path = sim_value::access::field(&intent, "path").expect("edit-field carries a path");
        // The edit is scoped to the focused field, never the root resource.
        assert_ne!(
            path,
            &Expr::List(Vec::new()),
            "char edit must not overwrite the root path []"
        );
        let parsed = sim_value::path::Path::from_expr(path).expect("path parses");
        assert_eq!(
            parsed,
            sim_value::path::Path::new().key(Expr::String("title".to_owned())),
            "char edit binds to the focused field key"
        );
    }

    #[test]
    fn char_without_focused_field_does_not_edit_root() {
        assert!(
            intent_from_key(&KeyInput::Char('x'), "main", "node-1", "", 7).is_none(),
            "a char with no focused field must not clobber the root value"
        );
    }

    #[test]
    fn colon_maps_to_invoke() {
        assert_valid(&KeyInput::Colon("quit".to_owned()), "invoke");
    }

    #[test]
    fn escape_maps_to_cancel() {
        let intent = assert_valid(&KeyInput::Escape, "cancel");
        assert_eq!(sim_value::access::field_str(&intent, "pane"), Some("main"));
    }

    #[test]
    fn backspace_has_no_mapping() {
        assert!(intent_from_key(&KeyInput::Backspace, "main", "node-1", "value", 7).is_none());
    }

    fn palette_commands() -> Vec<Command> {
        use sim_kernel::Symbol;
        use sim_lib_view::palette::CommandKind;
        vec![
            Command {
                id: Symbol::new("run"),
                label: "Run validation".to_owned(),
                kind: CommandKind::Invoke,
            },
            Command {
                id: Symbol::new("open-readme"),
                label: "Open README".to_owned(),
                kind: CommandKind::Open,
            },
        ]
    }

    #[test]
    fn palette_render_is_deterministic_ascii() {
        let commands = palette_commands();
        let first = render_palette(&commands, "");
        let second = render_palette(&commands, "");
        assert_eq!(first, second, "palette render must be deterministic");
        assert!(first.is_ascii(), "palette render must be ASCII");
        assert_eq!(first, ["[Run validation]", "[Open README]"].join("\n"));
        // Filtering narrows the rendered overlay deterministically.
        assert_eq!(render_palette(&commands, "open"), "[Open README]");
    }

    #[test]
    fn tui_and_web_palette_intent_are_identical() {
        let commands = palette_commands();
        // The TUI reaches the palette model through the `:`-prompt helper.
        let via_tui = palette_intent_from_colon(&commands, "run", "main", 3)
            .expect("colon entry selects a command");
        // The Web UI reaches the SAME shared model directly.
        let via_web = sim_lib_view::palette::palette_intent(&commands[0], "main", 3)
            .expect("command reduces");
        assert_eq!(via_tui, via_web, "both surfaces drive one palette model");
        validate_intent(&via_tui).expect("shared palette intent validates");
    }

    #[test]
    fn cli_and_tui_caps_differ_in_input_and_color() {
        let cli = cli_caps("tty.local.1");
        let tui = tui_caps("tty.local.1");
        assert_eq!(cli.preset_name(), "cli");
        assert_eq!(tui.preset_name(), "tui");
        // The tui surface accepts pointer input; the cli surface does not.
        assert!(!cli.input_flag("pointer"));
        assert!(tui.input_flag("pointer"));
        // Both are keyboard surfaces and carry the requested client id.
        assert!(cli.input_flag("keyboard") && tui.input_flag("keyboard"));
        assert_eq!(cli.client_id, "tty.local.1");
    }
}
