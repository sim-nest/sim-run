use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
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
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let envelope = args
            .values()
            .first()
            .ok_or_else(|| Error::Eval("missing JVM envelope".into()))?;
        let args = envelope_args(cx, envelope)?;
        let request = parse_jvm_args(&args)?;
        let outcome = sim_lib_lang_jvm::JvmSurface::new(1 << 20).execute_i32(cx, request);
        match outcome {
            sim_lib_lang_jvm::JvmExecutionOutcome::Value(value) => println!("JVM value: {value}"),
            sim_lib_lang_jvm::JvmExecutionOutcome::Throwable(throwable) => {
                println!("JVM throwable: {:?}", throwable.condition())
            }
            sim_lib_lang_jvm::JvmExecutionOutcome::Refusal(reason) => {
                println!("JVM refusal: {reason}")
            }
        }
        cx.factory().bool(true)
    }
}

fn parse_jvm_args(args: &[String]) -> Result<sim_lib_lang_jvm::JvmExecutionRequest> {
    let args = args.strip_prefix(&[JVM_VERB.to_owned()]).unwrap_or(args);
    let [classfile, class, member, descriptor, arguments @ ..] = args else {
        return Err(Error::Eval(
            "usage: sim jvm HEX_CLASSFILE CLASS MEMBER DESCRIPTOR [I32 ...]".into(),
        ));
    };
    let classfile = decode_hex(classfile)?;
    let arguments = arguments
        .iter()
        .map(|arg| {
            arg.parse::<i32>()
                .map_err(|_| Error::Eval(format!("invalid JVM integer argument: {arg}")))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(sim_lib_lang_jvm::JvmExecutionRequest {
        classfile,
        class: class.clone(),
        member: member.clone(),
        descriptor: descriptor.clone(),
        arguments,
    })
}

fn decode_hex(input: &str) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return Err(Error::Eval(
            "JVM classfile hex must contain complete bytes".into(),
        ));
    }
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("hex is ASCII-sized");
            u8::from_str_radix(text, 16)
                .map_err(|_| Error::Eval("JVM classfile is not hexadecimal".into()))
        })
        .collect()
}

fn envelope_args(cx: &mut Cx, envelope: &Value) -> Result<Vec<String>> {
    let table = envelope
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("JVM CLI envelope is not a table".into()))?;
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
            _ => Err(Error::TypeMismatch {
                expected: "string argument",
                found: "non-string",
            }),
        })
        .collect()
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
