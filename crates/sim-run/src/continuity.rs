use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use sha2::{Digest, Sha256};
use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession, cli_main_entrypoint_symbol};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const VERB: &str = "continuity";
const COMMAND_HOST: &str = "lib/continuity-command";
const DEFAULT_PLANS: &str = include_str!("continuity_plans.toml");

/// Static linkage is deliberately expressed as data. Adding a provider changes
/// this table, not bootloader parsing or a platform/device discriminant.
const PROVIDERS: &[Provider] = &[
    Provider::required("continuity-plan", "data/continuity-plan"),
    Provider::required("continuity-organ", "lib/continuity-organ"),
    Provider::required("android-capsule", "lib/platform-android"),
    Provider::required("android-root", "site/android-root"),
    Provider::required("phone-surface", "surface/phone"),
    Provider::required("phone-review", "service/phone-review"),
    Provider::required("lifecycle", "service/lifecycle"),
    Provider::required("mounts", "service/mounts"),
    Provider::required("capture", "service/capture"),
    Provider::required("render", "service/render"),
    Provider::required("stop", "service/stop"),
    Provider::required("journal-append", "service/journal-append"),
    Provider::optional("watch", "provider/watch"),
    Provider::optional("desk-display", "provider/desk-display"),
    Provider::optional("halo", "provider/halo"),
];

#[derive(Clone, Copy)]
struct Provider {
    role: &'static str,
    library: &'static str,
    required: bool,
}

impl Provider {
    const fn required(role: &'static str, library: &'static str) -> Self {
        Self {
            role,
            library,
            required: true,
        }
    }
    const fn optional(role: &'static str, library: &'static str) -> Self {
        Self {
            role,
            library,
            required: false,
        }
    }
}

pub(crate) fn with_continuity_if_selected(
    command: &CliCommand,
    mut session: LoadSession,
) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session.add_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec));
    session.add_host_factory(COMMAND_HOST, || Box::new(ContinuityCommandLib));
    for provider in PROVIDERS {
        let library = provider.library;
        session.add_host_factory(library, move || Box::new(ComponentLib { id: library }));
    }
    let mut sources = vec![
        LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
        LibSourceSpec::Host(COMMAND_HOST.into()),
    ];
    sources.extend(
        PROVIDERS
            .iter()
            .map(|provider| LibSourceSpec::Host(provider.library.into())),
    );
    session.with_default_verb_sources(VERB, sources)
}

struct ComponentLib {
    id: &'static str,
}

impl Lib for ComponentLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: symbol(self.id),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: vec![],
        }
    }
    fn load(&self, _: &mut LoadCx, _: &mut Linker<'_>) -> Result<()> {
        Ok(())
    }
}

struct ContinuityCommandLib;

impl Lib for ContinuityCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: symbol(COMMAND_HOST),
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
            cx.factory().opaque(Arc::new(ContinuityEntrypoint))?,
        )?;
        Ok(())
    }
}

struct ContinuityEntrypoint;

impl Object for ContinuityEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/continuity".into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ContinuityEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ContinuityEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let argv = command_args(cx, &args)?;
        let request = Request::parse(&argv).map_err(Error::Eval)?;
        let output = execute(&request).map_err(Error::Eval)?;
        print!("{output}");
        cx.factory().bool(true)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Request {
    plan: String,
    plan_file: Option<PathBuf>,
    mode: Mode,
    artifact_dir: Option<PathBuf>,
    dry_run: bool,
    absent: Vec<String>,
    event: Event,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Android,
    HostModeled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Help,
    Boot,
    Shutdown,
    Restart,
    Replay,
}

impl Request {
    fn parse(argv: &[String]) -> std::result::Result<Self, String> {
        let mut request = Self {
            plan: "continuity/carrier-only".into(),
            plan_file: None,
            mode: Mode::HostModeled,
            artifact_dir: None,
            dry_run: false,
            absent: vec![],
            event: Event::Boot,
        };
        let mut i = 1;
        while i < argv.len() {
            match argv[i].as_str() {
                "--help" | "-h" => {
                    request.event = Event::Help;
                    i += 1;
                }
                "--dry-run" => {
                    request.dry_run = true;
                    i += 1;
                }
                "--plan" => {
                    request.plan = take(argv, &mut i, "--plan")?;
                }
                "--plan-file" => {
                    request.plan_file = Some(take(argv, &mut i, "--plan-file")?.into());
                }
                "--mode" => {
                    request.mode = match take(argv, &mut i, "--mode")?.as_str() {
                        "android" => Mode::Android,
                        "host-modeled" => Mode::HostModeled,
                        value => return Err(format!("unsupported continuity mode: {value}")),
                    };
                }
                "--artifact-dir" => {
                    request.artifact_dir = Some(take(argv, &mut i, "--artifact-dir")?.into());
                }
                "--absent" => {
                    request.absent.push(take(argv, &mut i, "--absent")?);
                }
                "shutdown" => {
                    request.event = Event::Shutdown;
                    i += 1;
                }
                "restart" => {
                    request.event = Event::Restart;
                    i += 1;
                }
                "replay" => {
                    request.event = Event::Replay;
                    i += 1;
                }
                value => return Err(format!("unknown continuity argument: {value}")),
            }
        }
        if request.event != Event::Help && !request.dry_run && request.artifact_dir.is_none() {
            return Err("continuity requires --dry-run or --artifact-dir PATH".into());
        }
        Ok(request)
    }
}

fn execute(request: &Request) -> std::result::Result<String, String> {
    if request.event == Event::Help {
        return Ok(help().into());
    }
    let text = match &request.plan_file {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("read continuity plan {}: {error}", path.display()))?,
        None => DEFAULT_PLANS.into(),
    };
    let plans = parse_plans(&text)?;
    let plan = resolve_plan(&plans, &request.plan)?;
    let providers = PROVIDERS
        .iter()
        .map(|provider| (provider.role, *provider))
        .collect::<BTreeMap<_, _>>();
    for role in &plan.required {
        let provider = providers.get(role.as_str()).ok_or_else(|| {
            format!("required continuity service has no registered provider: {role}")
        })?;
        if !provider.required {
            return Err(format!(
                "continuity plan declares optional provider as required: {role}"
            ));
        }
        if request.absent.iter().any(|absent| absent == role) {
            return Err(format!(
                "required continuity service is absent: {}",
                provider.role
            ));
        }
    }
    let optional_absent = plan
        .optional
        .iter()
        .filter(|role| request.absent.iter().any(|absent| absent == *role))
        .cloned()
        .collect::<Vec<_>>();
    let mode = match request.mode {
        Mode::Android => "android",
        Mode::HostModeled => "host-modeled",
    };
    let event = match request.event {
        Event::Help => unreachable!("help returns before composition"),
        Event::Boot => "boot",
        Event::Shutdown => "shutdown",
        Event::Restart => "restart",
        Event::Replay => "replay",
    };
    let provider_rows = PROVIDERS
        .iter()
        .filter(|provider| !request.absent.iter().any(|absent| absent == provider.role))
        .map(|provider| format!("{}={}", provider.role, provider.library))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "schema=sim.continuity-artifact/v1\nplan={}\nrevision={}\nmode={mode}\nroot={}\nevent={event}\nproviders={provider_rows}\nunsigned=true\n",
        plan.id, plan.revision, plan.root
    );
    let digest = format!("sha256:{:x}", Sha256::digest(body.as_bytes()));
    if let Some(dir) = &request.artifact_dir {
        fs::create_dir_all(dir).map_err(|error| format!("create artifact directory: {error}"))?;
        let name = match request.mode {
            Mode::Android => "continuity.android.bundle",
            Mode::HostModeled => "continuity.host-modeled.bundle",
        };
        fs::write(dir.join(name), &body)
            .map_err(|error| format!("write continuity artifact: {error}"))?;
    }
    Ok(format!(
        "continuity: plan={} revision={} mode={mode} event={event} root={}\nartifact: {digest} unsigned\noptional-absent: {}\n",
        plan.id,
        plan.revision,
        plan.root,
        if optional_absent.is_empty() {
            "none".into()
        } else {
            optional_absent.join(",")
        }
    ))
}

#[derive(Clone, Debug)]
struct Plan {
    id: String,
    revision: u64,
    root: String,
    extends: Option<String>,
    required: Vec<String>,
    optional: Vec<String>,
}

fn parse_plans(text: &str) -> std::result::Result<Vec<Plan>, String> {
    if !text
        .lines()
        .any(|line| line.trim() == "schema = \"sim.continuity-plans/v1\"")
    {
        return Err("unsupported continuity plan schema".into());
    }
    let mut plans = Vec::new();
    let mut current: Option<Plan> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[plan]]" {
            if let Some(plan) = current.take() {
                plans.push(plan);
            }
            current = Some(Plan {
                id: String::new(),
                revision: 0,
                root: String::new(),
                extends: None,
                required: vec![],
                optional: vec![],
            });
        } else if let Some(plan) = current.as_mut() {
            if let Some(value) = string_field(line, "id") {
                plan.id = value;
            } else if let Some(value) = integer_field(line, "revision") {
                plan.revision = value?;
            } else if let Some(value) = string_field(line, "root") {
                plan.root = value;
            } else if let Some(value) = string_field(line, "extends") {
                plan.extends = Some(value);
            } else if let Some(value) = list_field(line, "required") {
                plan.required = value?;
            } else if let Some(value) = list_field(line, "optional_roles") {
                plan.optional = value?;
            }
        }
    }
    if let Some(plan) = current {
        plans.push(plan);
    }
    if plans.is_empty()
        || plans
            .iter()
            .any(|plan| plan.id.is_empty() || plan.revision == 0)
    {
        return Err("invalid continuity plan data".into());
    }
    Ok(plans)
}

fn resolve_plan(plans: &[Plan], id: &str) -> std::result::Result<Plan, String> {
    let mut plan = plans
        .iter()
        .find(|plan| plan.id == id)
        .cloned()
        .ok_or_else(|| format!("continuity plan not found: {id}"))?;
    if let Some(parent_id) = plan.extends.clone() {
        let parent = plans
            .iter()
            .find(|candidate| candidate.id == parent_id)
            .ok_or_else(|| format!("continuity parent plan not found: {parent_id}"))?;
        plan.root = parent.root.clone();
        plan.required = parent.required.clone();
    }
    if plan.root.is_empty() {
        return Err(format!("continuity plan has no root: {}", plan.id));
    }
    Ok(plan)
}

fn string_field(line: &str, key: &str) -> Option<String> {
    line.strip_prefix(&format!("{key} = \""))
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned)
}
fn integer_field(line: &str, key: &str) -> Option<std::result::Result<u64, String>> {
    line.strip_prefix(&format!("{key} = "))
        .map(|value| value.parse().map_err(|_| format!("invalid {key}")))
}
fn list_field(line: &str, key: &str) -> Option<std::result::Result<Vec<String>, String>> {
    line.strip_prefix(&format!("{key} = [")).map(|value| {
        let value = value
            .strip_suffix(']')
            .ok_or_else(|| format!("invalid {key}"))?;
        if value.trim().is_empty() {
            return Ok(vec![]);
        }
        value
            .split(',')
            .map(|item| {
                item.trim()
                    .strip_prefix('"')
                    .and_then(|item| item.strip_suffix('"'))
                    .map(str::to_owned)
                    .ok_or_else(|| format!("invalid {key}"))
            })
            .collect()
    })
}
fn symbol(value: &str) -> Symbol {
    value.split_once('/').map_or_else(
        || Symbol::new(value),
        |(namespace, name)| Symbol::qualified(namespace, name),
    )
}
fn take(argv: &[String], index: &mut usize, flag: &str) -> std::result::Result<String, String> {
    let value = argv
        .get(*index + 1)
        .ok_or_else(|| format!("{flag} requires a value"))?
        .clone();
    *index += 2;
    Ok(value)
}
fn command_args(cx: &mut Cx, args: &Args) -> Result<Vec<String>> {
    let envelope = args
        .values()
        .first()
        .ok_or_else(|| Error::Eval("missing continuity envelope".into()))?;
    let table = envelope
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("continuity envelope is not a table".into()))?;
    let Expr::List(items) = table.get(cx, Symbol::new("args"))?.object().as_expr(cx)? else {
        return Err(Error::Eval("continuity args are not a list".into()));
    };
    Ok(items
        .into_iter()
        .filter_map(|item| {
            if let Expr::String(value) = item {
                Some(value)
            } else {
                None
            }
        })
        .collect())
}
fn help() -> &'static str {
    "Usage: sim continuity [shutdown|restart|replay] [OPTIONS]\n\
     Options:\n  --plan ID\n  --plan-file PATH\n  --mode android|host-modeled\n  --dry-run\n  --artifact-dir PATH\n  --absent ROLE\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_data_changes_composition_without_provider_code() {
        let changed = DEFAULT_PLANS.replace("revision = 1", "revision = 7");
        let plans = parse_plans(&changed).unwrap();
        assert_eq!(
            resolve_plan(&plans, "continuity/carrier-only")
                .unwrap()
                .revision,
            7
        );
        assert_eq!(
            PROVIDERS
                .iter()
                .filter(|provider| provider.required)
                .count(),
            12
        );
    }

    #[test]
    fn static_registry_is_provider_data_not_platform_branching() {
        assert!(
            PROVIDERS
                .iter()
                .any(|provider| provider.library == "lib/platform-android")
        );
        assert!(
            PROVIDERS
                .iter()
                .any(|provider| provider.library == "surface/phone")
        );
        assert!(
            PROVIDERS
                .iter()
                .filter(|provider| !provider.required)
                .all(|provider| provider.library.starts_with("provider/"))
        );
    }
}
