use std::{ffi::OsString, fs, path::PathBuf, sync::Arc};

use sim_kernel::{
    AbiVersion, Callable, CatalogSource, Cx, Error, Export, Lib, LibLoader, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Symbol, Value, Version,
    library::LibSource as KernelLibSource, object::Args,
};

use crate::{
    CliBoot, CratesIoResolver, LibSourceSpec, LoadSession, Payload, cli_main_entrypoint_symbol,
};

fn manifest(id: &str, exports: Vec<Export>) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version("0.1.0".to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target: LibTarget::HostRegistered,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports,
    }
}

fn cli_main_export(symbol: Symbol) -> Export {
    Export::Function {
        symbol,
        function_id: None,
    }
}

fn codec_export(name: &str) -> Export {
    Export::Codec {
        symbol: Symbol::qualified("codec", name),
        codec_id: None,
    }
}

struct ScenarioLib {
    manifest: LibManifest,
    codec: Option<String>,
    entrypoint: Option<(Symbol, ScenarioEntrypoint)>,
    values: Vec<(Symbol, &'static str)>,
}

impl ScenarioLib {
    fn codec(name: &str, entrypoint: Option<ScenarioEntrypoint>) -> Self {
        let entrypoint_symbol = cli_main_entrypoint_symbol(&format!("codec-{name}"));
        let mut exports = vec![codec_export(name)];
        if entrypoint.is_some() {
            exports.push(cli_main_export(entrypoint_symbol.clone()));
        }
        Self {
            manifest: manifest(&format!("codec-{name}"), exports),
            codec: Some(name.to_owned()),
            entrypoint: entrypoint.map(|entrypoint| (entrypoint_symbol, entrypoint)),
            values: Vec::new(),
        }
    }

    fn command(id: &str, entrypoint: ScenarioEntrypoint) -> Self {
        let entrypoint_symbol = cli_main_entrypoint_symbol(id);
        Self {
            manifest: manifest(id, vec![cli_main_export(entrypoint_symbol.clone())]),
            codec: None,
            entrypoint: Some((entrypoint_symbol, entrypoint)),
            values: Vec::new(),
        }
    }

    fn value(id: &str, symbol: Symbol, display: &'static str) -> Self {
        Self {
            manifest: manifest(
                id,
                vec![Export::Value {
                    symbol: symbol.clone(),
                }],
            ),
            codec: None,
            entrypoint: None,
            values: vec![(symbol, display)],
        }
    }
}

impl Lib for ScenarioLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        if let Some(codec) = &self.codec {
            linker.codec_value(
                Symbol::qualified("codec", codec.as_str()),
                cx.factory().bool(true)?,
            )?;
        }
        if let Some((symbol, entrypoint)) = &self.entrypoint {
            linker.function_value(
                symbol.clone(),
                cx.factory().opaque(Arc::new(entrypoint.clone()))?,
            )?;
        }
        for (symbol, display) in &self.values {
            linker.value(symbol.clone(), cx.factory().string((*display).to_owned())?)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct ScenarioEntrypoint {
    expected: ExpectedEnvelope,
}

impl ScenarioEntrypoint {
    fn expecting(expected: ExpectedEnvelope) -> Self {
        Self { expected }
    }
}

impl Object for ScenarioEntrypoint {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("cli/main".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ScenarioEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ScenarioEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        let Some(envelope) = args.values().first() else {
            return Err(Error::Eval("missing envelope argument".to_owned()));
        };
        let matched = self.expected.matches(cx, envelope)?;
        cx.factory().bool(matched)
    }
}

#[derive(Clone)]
struct ExpectedEnvelope {
    codec: &'static str,
    verb: &'static str,
    args: Vec<&'static str>,
    eval: &'static str,
    script: &'static str,
    stdin: &'static str,
}

impl ExpectedEnvelope {
    fn matches(&self, cx: &mut Cx, envelope: &Value) -> sim_kernel::Result<bool> {
        let args = table_value(cx, envelope, "args")?;
        Ok(table_display(cx, envelope, "codec")? == self.codec
            && table_display(cx, envelope, "verb")? == self.verb
            && list_displays(cx, &args)? == self.args
            && table_display(cx, envelope, "eval")? == self.eval
            && table_display(cx, envelope, "script")? == self.script
            && table_display(cx, envelope, "stdin")? == self.stdin)
    }
}

struct ScenarioArtifactLoader;

impl LibLoader for ScenarioArtifactLoader {
    fn can_load(&self, source: &KernelLibSource) -> bool {
        matches!(source, KernelLibSource::Bytes(_) | KernelLibSource::Path(_))
    }

    fn load(&self, _cx: &mut Cx, source: KernelLibSource) -> sim_kernel::Result<Box<dyn Lib>> {
        artifact_lib(&artifact_bytes(source)?)
    }

    fn inspect_manifest(
        &self,
        _cx: &mut Cx,
        source: &KernelLibSource,
    ) -> sim_kernel::Result<Option<LibManifest>> {
        Ok(Some(artifact_manifest(&artifact_bytes_ref(source)?)?))
    }
}

#[test]
fn scenario_boot_lisp_eval_is_offline_and_stable() {
    let boot = CliBoot {
        payload: Payload {
            eval: Some("(+ 1 2)".to_owned()),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = LoadSession::new().with_host_factory("codec/lisp", || {
        Box::new(ScenarioLib::codec(
            "lisp",
            Some(ScenarioEntrypoint::expecting(ExpectedEnvelope {
                codec: "codec/lisp",
                verb: "nil",
                args: Vec::new(),
                eval: "(+ 1 2)",
                script: "nil",
                stdin: "nil",
            })),
        ))
    });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(session.receipts().len(), 1);
    assert_eq!(
        session.receipts()[0].requested_source,
        LibSourceSpec::Host("codec/lisp".to_owned())
    );
    assert_eq!(session.receipts()[0].manifest.id, Symbol::new("codec-lisp"));
}

#[test]
fn scenario_load_host_lib_dispatches_verb() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("scenario/host".to_owned())],
        payload: Payload {
            args: os_args(&["host-run", "--dry-run"]),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = scenario_session().with_host_factory("scenario/host", || {
        Box::new(ScenarioLib::command(
            "host-command",
            ScenarioEntrypoint::expecting(ExpectedEnvelope {
                codec: "codec/test",
                verb: "host-run",
                args: vec!["host-run", "--dry-run"],
                eval: "nil",
                script: "nil",
                stdin: "nil",
            }),
        ))
    });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        loaded_manifest_ids(&session),
        vec!["codec-test".to_owned(), "host-command".to_owned()]
    );
}

#[test]
fn scenario_load_bytes_lib_dispatches_verb() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Bytes(b"bytes-command".to_vec())],
        payload: Payload {
            args: os_args(&["bytes-run", "alpha"]),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = scenario_session();

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        loaded_manifest_ids(&session),
        vec!["codec-test".to_owned(), "bytes-command".to_owned()]
    );
}

#[test]
fn scenario_inspect_manifest_has_stable_output() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        inspect: Some("bytes:manifest-fixture".to_owned()),
        ..CliBoot::default()
    };
    let mut session = scenario_session();

    let output = session.run_loaded_introspection(&boot).unwrap();

    assert_eq!(
        output,
        "source bytes:16 bytes\n\
resolved bytes:16 bytes\n\
lib manifest-fixture\n\
version 0.1.0\n\
target HostRegistered\n\
exports:\n\
- kind=value symbol=manifest/value state=declared\n"
    );
}

#[test]
fn scenario_fake_crates_io_loads_without_network() {
    let cache = temp_cache("fake-crates-io");
    let artifact = temp_artifact("fake-crates-io");
    fs::write(&artifact, b"crate-command").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-lib-scenario",
        "0.1.0",
        artifact.clone(),
    );
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::CratesIo(
            "sim-lib-scenario@0.1.0".parse().unwrap(),
        )],
        payload: Payload {
            args: os_args(&["crate-run", "offline"]),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = scenario_session().with_crates_io_resolver(resolver);

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        loaded_manifest_ids(&session),
        vec!["codec-test".to_owned(), "crate-command".to_owned()]
    );
    assert!(matches!(
        session.receipts()[1].resolved_source,
        LibSourceSpec::Path(_)
    ));
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn scenario_server_verb_loads_from_catalog_without_baked_mode() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Symbol("server".to_owned())],
        payload: Payload {
            args: os_args(&["server", "--serve", "agent://planner"]),
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = scenario_session()
        .with_catalog_source("server", CatalogSource::Bytes(b"server-command".to_vec()));

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(
        loaded_manifest_ids(&session),
        vec!["codec-test".to_owned(), "server-command".to_owned()]
    );
}

#[test]
fn scenario_placement_report_is_observable_through_loaded_lib_inspect() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Bytes(b"placement-report".to_vec())],
        inspect: Some("placement/report".to_owned()),
        ..CliBoot::default()
    };
    let mut session = scenario_session();

    let output = session.run_loaded_introspection(&boot).unwrap();

    assert!(output.starts_with("export placement/report\n"), "{output}");
    assert!(
        output.contains("lib=placement-fixture kind=value symbol=placement/report state=resolved:"),
        "{output}"
    );
}

#[test]
fn recipe_commands_cover_deterministic_scenarios() {
    let root = source_root();
    let recipes = [
        (
            "boot-lisp-eval",
            "cargo test -p sim-run-core ",
            "scenario_boot_lisp_eval_is_offline_and_stable",
        ),
        (
            "host-verb",
            "cargo test -p sim-run-core ",
            "scenario_load_host_lib_dispatches_verb",
        ),
        (
            "bytes-lib",
            "cargo test -p sim-run-core ",
            "scenario_load_bytes_lib_dispatches_verb",
        ),
        (
            "inspect-manifest",
            "cargo test -p sim-run-core ",
            "scenario_inspect_manifest_has_stable_output",
        ),
        (
            "fake-crates-io",
            "cargo test -p sim-run-core ",
            "scenario_fake_crates_io_loads_without_network",
        ),
        (
            "server-verb",
            "cargo test -p sim-run-core ",
            "scenario_server_verb_loads_from_catalog_without_baked_mode",
        ),
        (
            "placement-report",
            "cargo test -p sim-run-core ",
            "scenario_placement_report_is_observable_through_loaded_lib_inspect",
        ),
        (
            "loadable-site",
            "cargo test --manifest-path \"$SIM_META_WORKSPACE_MANIFEST\" -p sim-lib-agent ",
            "loaded_site_resolves_through_model_at",
        ),
    ];

    for (recipe, command, test_name) in recipes {
        let dir = root.join("recipes/02-scenarios").join(recipe);
        let metadata = fs::read_to_string(dir.join("recipe.toml")).unwrap();
        let setup = fs::read_to_string(dir.join("setup.sh")).unwrap();
        assert!(metadata.contains("requires = ["), "{recipe}");
        assert!(metadata.contains("network = false"), "{recipe}");
        assert!(setup.contains(command), "{recipe}");
        assert!(setup.contains(test_name), "{recipe}");
    }
}

#[test]
fn config_recipe_commands_cover_report_scenarios() {
    let root = source_root();
    let recipes = [
        (
            "config-status",
            "cargo test -p sim-run-core ",
            "config_status_reports_loaded_libs_and_source_provenance",
        ),
        (
            "config-effective",
            "cargo test -p sim-run-core ",
            "effective_config_discovers_requested_per_lib_file",
        ),
        (
            "config-cookbook-override",
            "cargo test -p sim-run-core ",
            "effective_cookbook_override_reports_reordered_subset",
        ),
    ];

    for (recipe, command, test_name) in recipes {
        let dir = root.join("recipes/02-scenarios").join(recipe);
        let metadata = fs::read_to_string(dir.join("recipe.toml")).unwrap();
        let setup = fs::read_to_string(dir.join("setup.sh")).unwrap();
        assert!(metadata.contains("requires = ["), "{recipe}");
        assert!(metadata.contains("network = false"), "{recipe}");
        assert!(setup.contains(command), "{recipe}");
        assert!(setup.contains(test_name), "{recipe}");
    }
}

fn source_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in [
        manifest_dir.join("../.."),
        manifest_dir.join("../../../../sim-run"),
    ] {
        if candidate.join("recipes/book.toml").exists() {
            return candidate;
        }
    }
    panic!("could not locate sim-run source root from {manifest_dir:?}");
}

fn scenario_session() -> LoadSession {
    LoadSession::new()
        .with_loader(ScenarioArtifactLoader)
        .with_host_factory("codec/test", || Box::new(ScenarioLib::codec("test", None)))
}

fn artifact_lib(bytes: &[u8]) -> sim_kernel::Result<Box<dyn Lib>> {
    match bytes {
        b"bytes-command" => Ok(Box::new(ScenarioLib::command(
            "bytes-command",
            ScenarioEntrypoint::expecting(ExpectedEnvelope {
                codec: "codec/test",
                verb: "bytes-run",
                args: vec!["bytes-run", "alpha"],
                eval: "nil",
                script: "nil",
                stdin: "nil",
            }),
        ))),
        b"crate-command" => Ok(Box::new(ScenarioLib::command(
            "crate-command",
            ScenarioEntrypoint::expecting(ExpectedEnvelope {
                codec: "codec/test",
                verb: "crate-run",
                args: vec!["crate-run", "offline"],
                eval: "nil",
                script: "nil",
                stdin: "nil",
            }),
        ))),
        b"server-command" => Ok(Box::new(ScenarioLib::command(
            "server-command",
            ScenarioEntrypoint::expecting(ExpectedEnvelope {
                codec: "codec/test",
                verb: "server",
                args: vec!["server", "--serve", "agent://planner"],
                eval: "nil",
                script: "nil",
                stdin: "nil",
            }),
        ))),
        b"manifest-fixture" => Ok(Box::new(ScenarioLib::value(
            "manifest-fixture",
            Symbol::qualified("manifest", "value"),
            "manifest-ready",
        ))),
        b"placement-report" => Ok(Box::new(ScenarioLib::value(
            "placement-fixture",
            Symbol::qualified("placement", "report"),
            "(placement-report (node fx) (site local) (latency sample-exact))",
        ))),
        _ => Err(Error::Lib("scenario artifact rejected".to_owned())),
    }
}

fn artifact_manifest(bytes: &[u8]) -> sim_kernel::Result<LibManifest> {
    artifact_lib(bytes).map(|lib| lib.manifest())
}

fn artifact_bytes(source: KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    match source {
        KernelLibSource::Bytes(bytes) => Ok(bytes),
        KernelLibSource::Path(path) => {
            fs::read(path).map_err(|err| Error::Lib(format!("read artifact: {err}")))
        }
        _ => Err(Error::Lib("unsupported scenario source".to_owned())),
    }
}

fn artifact_bytes_ref(source: &KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    match source {
        KernelLibSource::Bytes(bytes) => Ok(bytes.clone()),
        KernelLibSource::Path(path) => {
            fs::read(path).map_err(|err| Error::Lib(format!("read artifact: {err}")))
        }
        _ => Err(Error::Lib("unsupported scenario source".to_owned())),
    }
}

fn table_value(cx: &mut Cx, table: &Value, field: &str) -> sim_kernel::Result<Value> {
    let Some(table) = table.object().as_table_impl() else {
        return Err(Error::Eval("envelope is not a table".to_owned()));
    };
    table.get(cx, Symbol::new(field))
}

fn table_display(cx: &mut Cx, table: &Value, field: &str) -> sim_kernel::Result<String> {
    let value = table_value(cx, table, field)?;
    value.object().display(cx)
}

fn list_displays(cx: &mut Cx, value: &Value) -> sim_kernel::Result<Vec<String>> {
    let Some(list) = value.object().as_list() else {
        return Err(Error::Eval("field is not a list".to_owned()));
    };
    list.to_vec(cx, Some(16))?
        .into_iter()
        .map(|value| value.object().display(cx))
        .collect()
}

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn loaded_manifest_ids(session: &LoadSession) -> Vec<String> {
    session
        .receipts()
        .iter()
        .map(|receipt| receipt.manifest.id.as_qualified_str())
        .collect()
}

fn temp_artifact(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-scenario-{}-{label}.artifact",
        std::process::id()
    ))
}

fn temp_cache(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-scenario-cache-{}-{label}",
        std::process::id()
    ))
}
