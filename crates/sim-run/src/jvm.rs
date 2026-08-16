use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Object,
    ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const JVM_VERB: &str = "jvm";
const JVM_RUNTIME_HOST: &str = "lib/lang-jvm";
const JVM_CLI_HOST: &str = "lib/lang-jvm-cli";

pub(crate) fn with_jvm_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_jvm_command(command) {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(JVM_RUNTIME_HOST, || {
            Box::new(sim_lib_lang_jvm::JvmLanguageLib::default())
        })
        .with_host_factory(JVM_CLI_HOST, || Box::new(JvmCliLib))
        .with_capability(sim_lib_lang_jvm::class_load_capability())
        .with_capability(sim_lib_lang_jvm::jvm_invoke_capability())
        .with_default_verb_sources(
            JVM_VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(JVM_RUNTIME_HOST.to_owned()),
                LibSourceSpec::Host(JVM_CLI_HOST.to_owned()),
            ],
        )
}

fn is_jvm_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == JVM_VERB)
}

struct JvmCliLib;

impl Lib for JvmCliLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::new(JVM_CLI_HOST),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: cli_main_entrypoint_symbol(JVM_VERB),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            cli_main_entrypoint_symbol(JVM_VERB),
            cx.factory().opaque(Arc::new(JvmEntrypoint))?,
        )?;
        Ok(())
    }
}

struct JvmEntrypoint;

impl Object for JvmEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/jvm".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for JvmEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for JvmEntrypoint {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        let report = sim_lib_lang_jvm::run_product_specimen(cx)?;
        println!(
            "JVM specimen: static={} object={} array={} exception={} concat={}",
            report.static_result,
            report.object_allocated,
            report.array_result,
            report.exception_class,
            report.concat_result
        );
        cx.factory().bool(true)
    }
}

#[cfg(test)]
mod tests {
    use sim_run_core::parse_args;

    use super::is_jvm_command;

    #[test]
    fn detects_jvm_product_verb() {
        assert!(is_jvm_command(&parse_args(["sim", "jvm"]).unwrap()));
        assert!(!is_jvm_command(&parse_args(["sim", "run"]).unwrap()));
    }
}
