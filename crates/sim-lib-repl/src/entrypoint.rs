use std::io;
use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, EagerPolicy, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, StrictNames, Symbol, Value, Version,
};
use sim_run_core::cli_main_entrypoint_symbol;

use crate::{eval_requested_text, run_repl_lines};

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
        prepare_repl_runtime(cx);
        let envelope = match args.values().first() {
            Some(envelope) => ReplEnvelope::from_value(cx, envelope)?,
            None => ReplEnvelope::default(),
        };
        let source = envelope.eval_source()?;
        let codec = envelope.codec.unwrap_or_else(default_codec);
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        if let Some(source) = source {
            let result = eval_requested_text(cx, &codec, &source).map_err(Error::Eval)?;
            use std::io::Write;
            writeln!(stdout, "{result}")
                .map_err(|err| Error::Eval(format!("write stdout: {err}")))?;
        } else {
            run_repl_lines(cx, &codec, stdin.lock(), &mut stdout).map_err(Error::Eval)?;
        }
        cx.factory().bool(true)
    }
}

fn prepare_repl_runtime(cx: &mut Cx) {
    if cx.eval_policy_name() == "noop" {
        cx.set_eval_policy(Arc::new(StrictNames(EagerPolicy)));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ReplEnvelope {
    codec: Option<Symbol>,
    verb: Option<String>,
    args: Vec<String>,
    eval: Option<String>,
}

impl ReplEnvelope {
    fn from_value(cx: &mut Cx, envelope: &Value) -> Result<Self> {
        Ok(Self {
            codec: envelope_codec(cx, envelope)?,
            verb: envelope_string(cx, envelope, "verb")?,
            args: envelope_args(cx, envelope)?,
            eval: envelope_string(cx, envelope, "eval")?,
        })
    }

    fn eval_source(&self) -> Result<Option<String>> {
        if let Some(eval) = &self.eval {
            return Ok(Some(eval.clone()));
        }
        if self.verb.as_deref() != Some("eval") {
            return Ok(None);
        }
        let source = self
            .args
            .iter()
            .skip(1)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if source.is_empty() {
            Err(Error::Eval("eval verb requires source text".to_owned()))
        } else {
            Ok(Some(source))
        }
    }
}

fn envelope_codec(cx: &mut Cx, envelope: &Value) -> Result<Option<Symbol>> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval("CLI envelope is not a table".to_owned()));
    };
    let value = table.get(cx, Symbol::new("codec"))?;
    match value.object().as_expr(cx)? {
        Expr::Symbol(symbol) => Ok(Some(symbol)),
        Expr::Nil => Ok(None),
        other => Err(Error::TypeMismatch {
            expected: "codec symbol",
            found: sim_value::kind::expr_kind(&other),
        }),
    }
}

fn envelope_string(cx: &mut Cx, envelope: &Value, name: &str) -> Result<Option<String>> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval("CLI envelope is not a table".to_owned()));
    };
    let value = table.get(cx, Symbol::new(name))?;
    match value.object().as_expr(cx)? {
        Expr::String(value) => Ok(Some(value)),
        Expr::Nil => Ok(None),
        other => Err(Error::TypeMismatch {
            expected: "string or nil",
            found: sim_value::kind::expr_kind(&other),
        }),
    }
}

fn envelope_args(cx: &mut Cx, envelope: &Value) -> Result<Vec<String>> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval("CLI envelope is not a table".to_owned()));
    };
    let value = table.get(cx, Symbol::new("args"))?;
    let Expr::List(items) = value.object().as_expr(cx)? else {
        return Err(Error::TypeMismatch {
            expected: "argument list",
            found: "non-list",
        });
    };
    items
        .into_iter()
        .map(|item| match item {
            Expr::String(value) => Ok(value),
            other => Err(Error::TypeMismatch {
                expected: "string argument",
                found: sim_value::kind::expr_kind(&other),
            }),
        })
        .collect()
}

fn default_codec() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

#[cfg(test)]
mod tests {
    use sim_kernel::{Export, Lib};

    use super::{ReplLib, repl_entrypoint_symbol};

    #[test]
    fn repl_lib_exports_cli_main_repl() {
        let lib = ReplLib::new();
        let manifest = lib.manifest();

        assert!(manifest.exports.iter().any(|export| matches!(
            export,
            Export::Function { symbol, .. } if symbol == &repl_entrypoint_symbol()
        )));
    }
}
