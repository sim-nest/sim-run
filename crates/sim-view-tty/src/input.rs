//! Reduce normalized terminal key events to validated [`Intent`] values.
//!
//! This is the testable boundary of the tty surface: raw termios bytes are
//! decoded elsewhere into a [`KeyInput`], and this module turns that normalized
//! event into a checked Intent on the bus. Keeping the reduction pure (a key
//! plus the focused pane/target/field plus a tick in, an `Option<Expr>` out)
//! lets the whole input surface be exercised without a terminal.
//!
//! Every returned Intent is built through [`sim_lib_intent::intent`] and then
//! re-checked with [`sim_lib_intent::validate_intent`]; a key that cannot form a
//! well-formed Intent yields `None` rather than an invalid value.

use sim_kernel::Expr;
use sim_lib_intent::{Origin, intent, validate_intent};
use sim_lib_view::palette::{Command, filter_commands, palette_intent};

/// A normalized terminal key event, decoded from raw input upstream.
///
/// This is the reduction layer the surface tests target: it names the keys the
/// tty surface acts on, not raw escape sequences. `Colon` carries the text of a
/// command-line entry (the part after the `:` prompt).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    /// The Enter/Return key: activate the focused target.
    Enter,
    /// The Up arrow: select toward the previous item.
    Up,
    /// The Down arrow: select toward the next item.
    Down,
    /// The Left arrow: move the focused node left.
    Left,
    /// The Right arrow: move the focused node right.
    Right,
    /// A printable character typed into the focused field. The field key is
    /// supplied to [`intent_from_key`]; with no focused field the keystroke is
    /// dropped rather than overwriting the whole resource.
    Char(char),
    /// The Backspace key (no Intent mapping; deletion is handled in the field).
    Backspace,
    /// A submitted command line (`:`-prompt), carrying the command text.
    Colon(String),
    /// The Escape key: cancel the active pane.
    Escape,
}

/// Reduces a key event to a validated Intent for `pane`/`target` at `tick`.
///
/// `field` names the focused field key within `target`'s resource; an
/// `intent/edit-field` is scoped to that field path so a keystroke edits one
/// field rather than overwriting the whole resource at the root path `[]`. When
/// `field` is empty there is no focused field to bind to, so [`KeyInput::Char`]
/// yields `None` instead of clobbering the root value.
///
/// Returns `None` for keys with no Intent mapping (currently [`KeyInput::Backspace`]),
/// for a [`KeyInput::Char`] with no focused `field`, and, defensively, for any
/// event that fails to form a valid Intent. The mappings are:
///
/// - [`KeyInput::Enter`] -> `intent/invoke` activating the focused target.
/// - [`KeyInput::Up`]/[`KeyInput::Down`] -> `intent/select` of the target.
/// - [`KeyInput::Left`]/[`KeyInput::Right`] -> `intent/move` of the target node.
/// - [`KeyInput::Char`] -> `intent/edit-field` typing into the focused `field`.
/// - [`KeyInput::Colon`] -> `intent/invoke` carrying the command string.
/// - [`KeyInput::Escape`] -> `intent/cancel` of the pane.
pub fn intent_from_key(
    key: &KeyInput,
    pane: &str,
    target: &str,
    field: &str,
    tick: u64,
) -> Option<Expr> {
    let origin = Origin::human(tick);
    let target_ref = Expr::String(target.to_owned());
    let built = match key {
        KeyInput::Enter => intent(
            "invoke",
            origin,
            vec![
                ("target", target_ref),
                ("op", sim_value::build::sym("activate")),
                ("args", Expr::List(Vec::new())),
            ],
        ),
        KeyInput::Up | KeyInput::Down => {
            let dir = if matches!(key, KeyInput::Up) {
                "up"
            } else {
                "down"
            };
            intent(
                "select",
                origin,
                vec![
                    ("targets", Expr::List(vec![target_ref])),
                    ("dir", sim_value::build::sym(dir)),
                ],
            )
        }
        KeyInput::Left | KeyInput::Right => {
            let dir = if matches!(key, KeyInput::Left) {
                "left"
            } else {
                "right"
            };
            intent(
                "move",
                origin,
                vec![("node", target_ref), ("at", sim_value::build::sym(dir))],
            )
        }
        KeyInput::Char(typed) => {
            if field.is_empty() {
                // No focused field: refuse to edit rather than overwriting the
                // entire resource at the root path `[]`.
                return None;
            }
            intent(
                "edit-field",
                origin,
                vec![
                    ("target", target_ref),
                    ("path", field_path(field)),
                    ("value", Expr::String(typed.to_string())),
                ],
            )
        }
        KeyInput::Colon(command) => intent(
            "invoke",
            origin,
            vec![
                ("target", target_ref),
                ("op", sim_value::build::sym("command")),
                ("args", Expr::List(vec![Expr::String(command.clone())])),
            ],
        ),
        KeyInput::Escape => intent(
            "cancel",
            origin,
            vec![("pane", Expr::String(pane.to_owned()))],
        ),
        // Backspace has no Intent mapping at the reduction layer.
        KeyInput::Backspace => return None,
    };
    validate_intent(&built).ok().map(|()| built)
}

/// Builds the single-key edit path to the focused `field` within the target,
/// in the shared `k`/`i` wire form the universal editor consumes.
fn field_path(field: &str) -> Expr {
    sim_value::path::Path::new()
        .key(Expr::String(field.to_owned()))
        .to_expr()
}

/// Drives the shared command palette from a `:`-prompt entry.
///
/// A [`KeyInput::Colon`] line is treated as a palette filter: the `command_line`
/// selects matching commands through [`filter_commands`] (the same predicate the
/// Web UI uses), and the first match is reduced to a validated Intent with
/// [`palette_intent`]. This is why the TUI and Web UI agree exactly: both reach
/// the same shared model.
///
/// Returns `None` when no command matches `command_line` (so the TUI can keep
/// the prompt open) or, defensively, when the chosen command fails to reduce.
pub fn palette_intent_from_colon(
    commands: &[Command],
    command_line: &str,
    pane: &str,
    tick: u64,
) -> Option<Expr> {
    let command = filter_commands(commands, command_line).into_iter().next()?;
    palette_intent(command, pane, tick).ok()
}
