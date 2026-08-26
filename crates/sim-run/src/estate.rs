// conformance: the estate boot verb maps only fixed subcommands to typed organ calls.

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};
use sim_estate_serve_alias::Call;
use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_lib_estate_serve as sim_estate_serve_alias;
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol};
use std::sync::Arc;

const VERB: &str = "estate";
const HOST: &str = "lib/estate-command";
const SUBCOMMANDS: &[&str] = &[
    "discover",
    "plan",
    "preview",
    "review",
    "apply",
    "watch",
    "verify",
    "reconcile",
    "history",
    "doctor",
];

pub(crate) fn with_estate_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|a| a == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || Box::new(EstateCommandLib))
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(HOST.into()),
            ],
        )
}

struct EstateCommandLib;
impl Lib for EstateCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "estate-command"),
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
            cx.factory().opaque(Arc::new(EstateEntrypoint))?,
        )?;
        Ok(())
    }
}
struct EstateEntrypoint;
impl Object for EstateEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/estate".into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for EstateEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for EstateEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let envelope = args
            .values()
            .first()
            .ok_or_else(|| Error::Eval("missing estate envelope".into()))?;
        let table = envelope
            .object()
            .as_table_impl()
            .ok_or_else(|| Error::Eval("estate envelope is not a table".into()))?;
        let Expr::List(items) = table.get(cx, Symbol::new("args"))?.object().as_expr(cx)? else {
            return Err(Error::Eval("estate args are not a list".into()));
        };
        let argv: Vec<String> = items
            .into_iter()
            .filter_map(|x| {
                if let Expr::String(s) = x {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        let sub = argv.get(1).map(String::as_str).unwrap_or("doctor");
        let call = compile_call(sub, &argv[2..]).map_err(Error::Eval)?;
        println!("estate organ call: {call:?}");
        cx.factory().bool(true)
    }
}
fn compile_call(sub: &str, args: &[String]) -> std::result::Result<Call, String> {
    let arg = |n: usize, name: &str| {
        args.get(n)
            .cloned()
            .ok_or_else(|| format!("estate {sub} requires {name}"))
    };
    let key = |s: String| sim_lib_estate_book::Key(s);
    Ok(match sub {
        "discover" => Call::Discover,
        "plan" => Call::Plan {
            operation: arg(0, "operation")?,
        },
        "preview" => Call::Preview {
            plan: key(arg(0, "plan key")?),
        },
        "review" => Call::Review {
            plan: key(arg(0, "plan key")?),
        },
        "apply" => Call::Apply {
            plan: key(arg(0, "plan key")?),
            approval: key(arg(1, "approval key")?),
        },
        "watch" => Call::Watch {
            run: arg(0, "run")?,
        },
        "verify" => Call::Verify {
            run: arg(0, "run")?,
        },
        "reconcile" => Call::Reconcile {
            run: arg(0, "run")?,
        },
        "history" => Call::History {
            run: args.first().cloned(),
        },
        "doctor" => Call::Doctor,
        _ => {
            return Err(format!(
                "unknown estate command: {sub}; expected {}",
                SUBCOMMANDS.join("|")
            ));
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exposes_exact_verbs_as_typed_calls() {
        for sub in SUBCOMMANDS {
            let args = match *sub {
                "plan" => vec!["op".into()],
                "preview" | "review" => vec!["plan".into()],
                "apply" => vec!["plan".into(), "approval".into()],
                "watch" | "verify" | "reconcile" => vec!["run".into()],
                _ => vec![],
            };
            assert!(compile_call(sub, &args).is_ok(), "{sub}");
        }
    }
    #[test]
    fn command_text_is_not_a_verb() {
        assert!(compile_call("shell", &["rm".into()]).is_err());
    }
}
