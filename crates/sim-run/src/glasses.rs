use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{
    CliCommand, DeviceHostStalePolicy, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol,
    compose_device_host,
};

use crate::{
    glasses_args::GlassesArgs,
    glasses_plan::{GlassesDevicePlan, GlassesPlan},
};

const GLASSES_VERB: &str = "glasses";
const GLASSES_APP_HOST: &str = "app/glasses";
const GLASSES_BOOT_CODEC_HOST: &str = "codec/lisp";

pub(crate) fn with_glasses_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_glasses_command(command) {
        return session;
    }
    session
        .with_host_factory(GLASSES_BOOT_CODEC_HOST, || Box::new(GlassesBootCodec))
        .with_host_factory(GLASSES_APP_HOST, || Box::new(GlassesLib))
        .with_default_verb_sources(
            GLASSES_VERB,
            vec![
                LibSourceSpec::Host(GLASSES_BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(GLASSES_APP_HOST.to_owned()),
            ],
        )
}

fn is_glasses_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == GLASSES_VERB)
}

struct GlassesBootCodec;

impl Lib for GlassesBootCodec {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("codec", "lisp"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: Symbol::qualified("codec", "lisp"),
                codec_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.codec_value(Symbol::qualified("codec", "lisp"), cx.factory().bool(true)?)?;
        Ok(())
    }
}

struct GlassesLib;

impl Lib for GlassesLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("app", "glasses"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: glasses_entrypoint_symbol(),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            glasses_entrypoint_symbol(),
            cx.factory().opaque(Arc::new(GlassesEntrypoint))?,
        )?;
        Ok(())
    }
}

fn glasses_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol(GLASSES_VERB)
}

#[derive(Clone)]
struct GlassesEntrypoint;

impl Object for GlassesEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/glasses".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for GlassesEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for GlassesEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let Some(envelope) = args.values().first() else {
            return Err(Error::Eval("missing glasses envelope".to_owned()));
        };
        let args = envelope_args(cx, envelope)?;
        let glasses = GlassesArgs::parse(&args)?;
        if glasses.help {
            print!("{GLASSES_HELP}");
            return cx.factory().bool(true);
        }
        let plan = glasses.plan()?;
        let mut sessions = Vec::with_capacity(plan.devices.len());
        for device in &plan.devices {
            let session = compose_device_host(cx, device.spec.clone())?;
            print_ready(device, &session);
            sessions.push(session);
        }
        print_plan_ready(&plan);
        cx.factory().bool(true)
    }
}

const GLASSES_HELP: &str = "\
Usage: sim glasses [OPTIONS]

Options:
  --device viture|halo|auto|both
  --profile auto|luma-ultra|halo|display-only|neckband
  --route direct-linux|android-usb|neckband-local|neckband-relay|mobile-dock-display|ble-direct|web-bluetooth|phone-relay|controller-hid
  --pose auto|none|modeled|viture|halo
  --mirror
  --encoder host|neckband|peer
  --display auto|N
  --layout TABLE-PATH
  --vendor-report never|ask|allow
  --dry-run
";

fn print_ready(device: &GlassesDevicePlan, session: &sim_run_core::DeviceEdgeSession) {
    println!(
        "glasses: {:<6} profile={} tier={} adapter={} loop={} stale={}",
        device.label,
        device.profile_label,
        device.tier_label,
        device.adapter_label,
        device.loop_label,
        stale_label(session.stale_policy())
    );
    if let Some(fallback) = &device.fallback {
        println!(
            "glasses: {:<6} route={} fallback=modeled reason={}",
            device.label, fallback.route, fallback.reason
        );
    }
}

fn print_plan_ready(plan: &GlassesPlan) {
    if plan.co_use {
        println!("glasses: co-use hub ready; boot plan ready");
    } else {
        println!("glasses: boot plan ready");
    }
}

fn stale_label(stale: DeviceHostStalePolicy) -> &'static str {
    match stale {
        DeviceHostStalePolicy::HoldLast => "hold-last",
        DeviceHostStalePolicy::PredictClamp => "predict+clamp",
        DeviceHostStalePolicy::Blank => "blank",
        DeviceHostStalePolicy::Refuse => "refuse",
    }
}

fn envelope_args(cx: &mut Cx, envelope: &Value) -> Result<Vec<String>> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval(
            "glasses CLI envelope is not a table".to_owned(),
        ));
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

    use super::is_glasses_command;

    #[test]
    fn detects_glasses_payload_verb() {
        let command = parse_args(["sim", "glasses", "--dry-run"]).unwrap();
        assert!(is_glasses_command(&command));
    }

    #[test]
    fn non_glasses_payload_stays_on_default_boot_path() {
        let command = parse_args(["sim", "run"]).unwrap();
        assert!(!is_glasses_command(&command));
    }
}
