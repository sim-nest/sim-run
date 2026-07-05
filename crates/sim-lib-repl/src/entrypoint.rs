use std::io;
use std::sync::Arc;

use sim_run_core::cli_main_entrypoint_symbol;
use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::run_repl_lines;

/// Returns the function symbol exported for the bootloader handoff.
pub fn repl_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol("repl")
}

/// Loadable REPL library.
#[derive(Clone, Debug, Default)]
pub struct ReplLib;

impl ReplLib {
    /// Creates a REPL library instance.
    pub fn new() -> Self {
        Self
    }
}

impl Lib for ReplLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "repl"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: repl_entrypoint_symbol(),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let entrypoint = cx.factory().opaque(Arc::new(ReplEntrypoint))?;
        linker.function_value(repl_entrypoint_symbol(), entrypoint)?;
        Ok(())
    }
}

#[derive(Clone)]
struct ReplEntrypoint;

impl Object for ReplEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/repl".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ReplEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ReplEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let codec = match args.values().first() {
            Some(envelope) => envelope_codec(cx, envelope)?,
            None => default_codec(),
        };
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        run_repl_lines(cx, &codec, stdin.lock(), &mut stdout).map_err(Error::Eval)?;
        cx.factory().bool(true)
    }
}

fn envelope_codec(cx: &mut Cx, envelope: &Value) -> Result<Symbol> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval("CLI envelope is not a table".to_owned()));
    };
    let value = table.get(cx, Symbol::new("codec"))?;
    match value.object().as_expr(cx)? {
        Expr::Symbol(symbol) => Ok(symbol),
        Expr::Nil => Ok(default_codec()),
        other => Err(Error::TypeMismatch {
            expected: "codec symbol",
            found: expr_kind(&other),
        }),
    }
}

fn default_codec() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::Nil => "nil",
        Expr::Bool(_) => "bool",
        Expr::Number(_) => "number",
        Expr::String(_) => "string",
        Expr::Bytes(_) => "bytes",
        Expr::Symbol(_) => "symbol",
        Expr::Vector(_) => "vector",
        Expr::List(_) => "list",
        Expr::Map(_) => "map",
        Expr::Set(_) => "set",
        Expr::Call { .. } => "call",
        Expr::Infix { .. } => "infix",
        Expr::Prefix { .. } => "prefix",
        Expr::Postfix { .. } => "postfix",
        Expr::Block(_) => "block",
        Expr::Quote { .. } => "quote",
        Expr::Annotated { .. } => "annotated",
        Expr::Local(_) => "local",
        Expr::Extension { .. } => "extension",
    }
}

#[cfg(test)]
mod tests {
    use sim::kernel::Lib;

    use super::{ReplLib, repl_entrypoint_symbol};

    #[test]
    fn repl_lib_exports_cli_main_repl() {
        let lib = ReplLib::new();
        let manifest = lib.manifest();

        assert!(manifest.exports.iter().any(|export| matches!(
            export,
            sim::kernel::Export::Function { symbol, .. } if symbol == &repl_entrypoint_symbol()
        )));
    }
}
