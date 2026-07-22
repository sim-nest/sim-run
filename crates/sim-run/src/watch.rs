use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{
    CliCommand, DeviceHostStalePolicy, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol,
    compose_device_host,
};

use crate::watch_args::{WatchArgs, WatchPlan};

const WATCH_VERB: &str = "watch";
const WATCH_APP_HOST: &str = "app/watch";
const WATCH_BOOT_CODEC_HOST: &str = "codec/lisp";

pub(crate) fn with_watch_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_watch_command(command) {
        return session;
    }
    session
        .with_host_factory(WATCH_BOOT_CODEC_HOST, || Box::new(WatchBootCodec))
        .with_host_factory(WATCH_APP_HOST, || Box::new(WatchLib))
        .with_default_verb_sources(
            WATCH_VERB,
            vec![
                LibSourceSpec::Host(WATCH_BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(WATCH_APP_HOST.to_owned()),
            ],
        )
}

fn is_watch_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == WATCH_VERB)
}

struct WatchBootCodec;

impl Lib for WatchBootCodec {
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

struct WatchLib;

impl Lib for WatchLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("app", "watch"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: watch_entrypoint_symbol(),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            watch_entrypoint_symbol(),
            cx.factory().opaque(Arc::new(WatchEntrypoint))?,
        )?;
        Ok(())
    }
}

fn watch_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol(WATCH_VERB)
}

#[derive(Clone)]
struct WatchEntrypoint;

impl Object for WatchEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/watch".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for WatchEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for WatchEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let Some(envelope) = args.values().first() else {
            return Err(Error::Eval("missing watch envelope".to_owned()));
        };
        let args = envelope_args(cx, envelope)?;
        let watch = WatchArgs::parse(&args)?;
        if watch.help {
            print!("{WATCH_HELP}");
            return cx.factory().bool(true);
        }
        let plan = watch.plan()?;
        if !plan.unavailable.is_empty() {
            print_unavailable(&plan);
            if !watch.dry_run {
                return Err(Error::Eval(plan.unavailable.join("; ")));
            }
        } else {
            let session = compose_device_host(cx, plan.spec.clone())?;
            print_ready(&watch, &plan, &session);
        }
        cx.factory().bool(true)
    }
}

const WATCH_HELP: &str = "\
Usage: sim watch [OPTIONS]

Options:
  --profile watch-glance|watch-glance-large|watch-sport|watch-sleep
  --route modeled|import|ble|relay|zepp-bridge|wifi-local
  --source modeled|import <file>|live
  --fleet one|pair
  --consent PATH
  --asr-site REF
  --vendor-report never|ask|allow
  --dry-run
";

fn print_ready(watch: &WatchArgs, plan: &WatchPlan, session: &sim_run_core::DeviceEdgeSession) {
    println!(
        "watch: profile={} tier={} source={} route={}",
        watch.profile.as_str(),
        watch.profile.tier(),
        watch.source.as_str(),
        plan.route.as_str()
    );
    println!(
        "watch: rate={} stale={} glance-adapter={}",
        watch.profile.rate_label(),
        stale_label(session.stale_policy()),
        plan.glance_adapter
    );
    println!("watch: boot plan ready");
}

fn print_unavailable(plan: &WatchPlan) {
    for reason in &plan.unavailable {
        println!("watch: unavailable {reason}");
    }
    println!("watch: boot plan unavailable");
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
        return Err(Error::Eval("watch CLI envelope is not a table".to_owned()));
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

    use super::is_watch_command;

    #[test]
    fn detects_watch_payload_verb() {
        let command = parse_args(["sim", "watch", "--dry-run"]).unwrap();
        assert!(is_watch_command(&command));
    }

    #[test]
    fn non_watch_payload_stays_on_default_boot_path() {
        let command = parse_args(["sim", "run"]).unwrap();
        assert!(!is_watch_command(&command));
    }
}
