//! Loadable library entrypoint for the terminal surface.
//!
//! The entrypoint wraps the pure render and input helpers as host-registered
//! callables. It does not own terminal IO; callers pass normalized scene/key
//! data and receive deterministic values back.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, NumberLiteral, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::{KeyInput, cli_caps, intent_from_key, render_scene, tui_caps};

const DEFAULT_CLIENT_ID: &str = "tty.loadable";

/// Returns the exported render function symbol.
pub fn tty_render_symbol() -> Symbol {
    Symbol::qualified("surface", "tty/render")
}

/// Returns the exported normalized-key-to-Intent function symbol.
pub fn tty_intent_symbol() -> Symbol {
    Symbol::qualified("surface", "tty/intent")
}

/// Loadable terminal view/edit surface library.
///
/// The library exports two pure functions:
///
/// - `surface/tty/render`: `(scene [preset] [client-id]) -> string`
/// - `surface/tty/intent`: `(key [pane] [target] [field] [tick]) -> intent|nil`
#[derive(Clone, Debug, Default)]
pub struct TtyViewLib;

impl TtyViewLib {
    /// Creates a terminal surface library instance.
    pub fn new() -> Self {
        Self
    }
}

impl Lib for TtyViewLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("view", "tty"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![
                Export::Function {
                    symbol: tty_render_symbol(),
                    function_id: None,
                },
                Export::Function {
                    symbol: tty_intent_symbol(),
                    function_id: None,
                },
            ],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            tty_render_symbol(),
            cx.factory().opaque(Arc::new(TtyRenderFn))?,
        )?;
        linker.function_value(
            tty_intent_symbol(),
            cx.factory().opaque(Arc::new(TtyIntentFn))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct TtyRenderFn;

impl Object for TtyRenderFn {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("surface/tty/render".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TtyRenderFn {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for TtyRenderFn {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        ensure_arg_range(&args, 1, 3, "surface/tty/render")?;
        let scene = required_expr(cx, &args, 0, "scene")?;
        let preset = optional_text(cx, &args, 1, "preset")?.unwrap_or_else(|| "cli".to_owned());
        let client_id = optional_text(cx, &args, 2, "client-id")?
            .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_owned());
        let caps = match preset.as_str() {
            "cli" => cli_caps(&client_id),
            "tui" => tui_caps(&client_id),
            other => {
                return Err(Error::Eval(format!(
                    "surface/tty/render preset must be 'cli' or 'tui', got {other}"
                )));
            }
        };
        cx.factory().string(render_scene(&scene, &caps))
    }
}

#[derive(Clone)]
struct TtyIntentFn;

impl Object for TtyIntentFn {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("surface/tty/intent".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TtyIntentFn {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for TtyIntentFn {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        ensure_arg_range(&args, 1, 5, "surface/tty/intent")?;
        let key = key_input_from_expr(&required_expr(cx, &args, 0, "key")?)?;
        let pane = optional_text(cx, &args, 1, "pane")?.unwrap_or_else(|| "main".to_owned());
        let target = optional_text(cx, &args, 2, "target")?.unwrap_or_else(|| "focused".to_owned());
        let field = optional_text(cx, &args, 3, "field")?.unwrap_or_default();
        let tick = optional_tick(cx, &args, 4)?.unwrap_or_default();
        match intent_from_key(&key, &pane, &target, &field, tick) {
            Some(intent) => cx.factory().expr(intent),
            None => cx.factory().nil(),
        }
    }
}

fn ensure_arg_range(args: &Args, min: usize, max: usize, name: &str) -> Result<()> {
    let count = args.values().len();
    if count < min || count > max {
        return Err(Error::Eval(format!(
            "{name} expects {min}..={max} arguments, got {count}"
        )));
    }
    Ok(())
}

fn required_expr(cx: &mut Cx, args: &Args, index: usize, label: &str) -> Result<Expr> {
    let Some(value) = args.values().get(index) else {
        return Err(Error::Eval(format!("missing {label} argument")));
    };
    value.object().as_expr(cx)
}

fn optional_text(
    cx: &mut Cx,
    args: &Args,
    index: usize,
    label: &'static str,
) -> Result<Option<String>> {
    let Some(value) = args.values().get(index) else {
        return Ok(None);
    };
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(Some(text)),
        Expr::Symbol(symbol) => Ok(Some(symbol.as_qualified_str())),
        other => Err(Error::TypeMismatch {
            expected: label,
            found: sim_value::kind::expr_kind(&other),
        }),
    }
}

fn optional_tick(cx: &mut Cx, args: &Args, index: usize) -> Result<Option<u64>> {
    let Some(value) = args.values().get(index) else {
        return Ok(None);
    };
    match value.object().as_expr(cx)? {
        Expr::Number(NumberLiteral { canonical, .. }) => {
            canonical.parse::<u64>().map(Some).map_err(|_| {
                Error::Eval(format!(
                    "tick must be a non-negative integer, got {canonical}"
                ))
            })
        }
        Expr::String(text) => text
            .parse::<u64>()
            .map(Some)
            .map_err(|_| Error::Eval(format!("tick must be a non-negative integer, got {text}"))),
        other => Err(Error::TypeMismatch {
            expected: "tick number",
            found: sim_value::kind::expr_kind(&other),
        }),
    }
}

fn key_input_from_expr(expr: &Expr) -> Result<KeyInput> {
    match expr {
        Expr::Symbol(symbol) => key_input_from_name(&symbol.as_qualified_str()),
        Expr::String(text) => string_key_input(text),
        Expr::Map(_) => mapped_key_input(expr),
        other => Err(Error::TypeMismatch {
            expected: "normalized key input",
            found: sim_value::kind::expr_kind(other),
        }),
    }
}

fn string_key_input(text: &str) -> Result<KeyInput> {
    if let Some(char_input) = single_char(text) {
        return Ok(KeyInput::Char(char_input));
    }
    key_input_from_name(text)
}

fn key_input_from_name(name: &str) -> Result<KeyInput> {
    let bare = name.rsplit('/').next().unwrap_or(name);
    match bare {
        "enter" => Ok(KeyInput::Enter),
        "up" => Ok(KeyInput::Up),
        "down" => Ok(KeyInput::Down),
        "left" => Ok(KeyInput::Left),
        "right" => Ok(KeyInput::Right),
        "backspace" => Ok(KeyInput::Backspace),
        "escape" => Ok(KeyInput::Escape),
        other => Err(Error::Eval(format!(
            "unknown normalized key input: {other}"
        ))),
    }
}

fn mapped_key_input(expr: &Expr) -> Result<KeyInput> {
    let kind = text_field(expr, "kind")
        .or_else(|| text_field(expr, "key"))
        .ok_or_else(|| Error::Eval("normalized key map requires kind or key".to_owned()))?;
    match kind.as_str() {
        "char" => {
            let text = text_field(expr, "char")
                .or_else(|| text_field(expr, "value"))
                .ok_or_else(|| Error::Eval("char key map requires char or value".to_owned()))?;
            let Some(value) = single_char(&text) else {
                return Err(Error::Eval(format!(
                    "char key map requires exactly one character, got {text}"
                )));
            };
            Ok(KeyInput::Char(value))
        }
        "colon" => {
            let text = text_field(expr, "text")
                .or_else(|| text_field(expr, "command"))
                .or_else(|| text_field(expr, "value"))
                .ok_or_else(|| {
                    Error::Eval("colon key map requires text, command, or value".to_owned())
                })?;
            Ok(KeyInput::Colon(text))
        }
        name => key_input_from_name(name),
    }
}

fn text_field(expr: &Expr, name: &str) -> Option<String> {
    match sim_value::access::field(expr, name)? {
        Expr::String(text) => Some(text.clone()),
        Expr::Symbol(symbol) => Some(symbol.as_qualified_str()),
        _ => None,
    }
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}
