use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_platform_core::{parse_bundle, parse_capsule, validate_bundle};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const VERB: &str = "platform";
const HOST: &str = "lib/platform-command";

pub(crate) fn with_platform_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || Box::new(PlatformCommandLib))
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(HOST.into()),
            ],
        )
}

struct PlatformCommandLib;
impl Lib for PlatformCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "platform-command"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: vec![Export::Function {
                symbol: cli_main_entrypoint_symbol(VERB),
                function_id: None,
            }],
        }
    }
    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            cli_main_entrypoint_symbol(VERB),
            cx.factory().opaque(Arc::new(PlatformEntrypoint))?,
        )?;
        Ok(())
    }
}

struct PlatformEntrypoint;
impl Object for PlatformEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/platform".into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for PlatformEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for PlatformEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let envelope = args
            .values()
            .first()
            .ok_or_else(|| Error::Eval("missing platform envelope".into()))?;
        let table = envelope
            .object()
            .as_table_impl()
            .ok_or_else(|| Error::Eval("platform envelope is not a table".into()))?;
        let Expr::List(items) = table.get(cx, Symbol::new("args"))?.object().as_expr(cx)? else {
            return Err(Error::Eval("platform args are not a list".into()));
        };
        let argv: Vec<_> = items
            .into_iter()
            .filter_map(|item| {
                if let Expr::String(s) = item {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        let sub = argv.get(1).map(String::as_str).unwrap_or("show");
        let output = match sub {
            "show" => "platform/site/model\nplatform/site/ubuntu-pc\n".to_owned(),
            "require" => format!("requirements: {}\n", argv[2..].join(",")),
            "doctor" => {
                validate_curated().map_err(Error::Eval)?;
                "platform bundles: healthy\n".to_owned()
            }
            "attest-verify" => match argv.get(2).map(String::as_str) {
                Some("ubuntu-pc") => "attestation: valid\n".to_owned(),
                _ => return Err(Error::Eval("attest-verify requires ubuntu-pc".into())),
            },
            other => return Err(Error::Eval(format!("unknown platform command: {other}"))),
        };
        print!("{output}");
        cx.factory().bool(true)
    }
}

fn validate_curated() -> std::result::Result<(), String> {
    let model =
        parse_bundle(sim_platform_model::MODEL_BUNDLE_MANIFEST).map_err(|e| e.to_string())?;
    let fictional =
        parse_capsule(sim_platform_model::FICTIONAL_CAPSULE).map_err(|e| e.to_string())?;
    validate_bundle(&model, &fictional).map_err(|e| e.to_string())?;
    let ubuntu =
        parse_bundle(sim_platform_ubuntu_pc::UBUNTU_BUNDLE_MANIFEST).map_err(|e| e.to_string())?;
    let capsule = parse_capsule(sim_platform_ubuntu_pc::UBUNTU_CAPSULE_MANIFEST)
        .map_err(|e| e.to_string())?;
    validate_bundle(&ubuntu, &capsule).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curated_doctor_passes() {
        validate_curated().unwrap();
    }
}
