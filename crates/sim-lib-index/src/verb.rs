//! Runtime library export for `cli/main/index`.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Lib, LibManifest, LibTarget, Linker, LoadCx,
    Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::cli_main_entrypoint_symbol;

use crate::{
    IndexDir, envelope::envelope_args, load_embedded_index_doc, parse_index_args, render_command,
};

/// Host-registered library that exports the SIM Index exploration entrypoint.
#[derive(Clone, Default)]
pub struct IndexLib;

impl IndexLib {
    /// Builds the host library.
    pub fn new() -> Self {
        Self
    }
}

impl Lib for IndexLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "index"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![
                Export::Function {
                    symbol: index_entrypoint_symbol(),
                    function_id: None,
                },
                Export::Value {
                    symbol: index_dir_symbol(),
                },
            ],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let doc = load_embedded_index_doc().map_err(|err| Error::Eval(err.to_string()))?;
        linker.value(
            index_dir_symbol(),
            cx.factory().opaque(Arc::new(IndexDir::new(doc)))?,
        )?;
        linker.function_value(
            index_entrypoint_symbol(),
            cx.factory().opaque(Arc::new(IndexEntrypoint))?,
        )?;
        Ok(())
    }
}

/// Symbol exported by the command entrypoint.
pub fn index_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol("index")
}

fn index_dir_symbol() -> Symbol {
    Symbol::qualified("index", "dir")
}

#[derive(Clone)]
struct IndexEntrypoint;

impl Object for IndexEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/index".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IndexEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for IndexEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let Some(envelope) = args.values().first() else {
            return Err(Error::Eval("missing index envelope".to_owned()));
        };
        let args = envelope_args(cx, envelope)?;
        let command = parse_index_args(&args).map_err(|err| Error::Eval(err.to_string()))?;
        let doc = load_embedded_index_doc().map_err(|err| Error::Eval(err.to_string()))?;
        let output = render_command(&command, &doc).map_err(|err| Error::Eval(err.to_string()))?;
        print!("{output}");
        cx.factory().bool(true)
    }
}

#[cfg(test)]
mod tests {
    use sim_kernel::Export;

    use super::{IndexLib, index_entrypoint_symbol};

    #[test]
    fn index_lib_exports_cli_main_index() {
        let manifest = sim_kernel::Lib::manifest(&IndexLib::new());

        assert!(manifest.exports.iter().any(|export| matches!(
            export,
            Export::Function { symbol, .. } if symbol == &index_entrypoint_symbol()
        )));
    }
}
