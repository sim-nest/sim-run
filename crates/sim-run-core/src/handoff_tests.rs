use std::{ffi::OsString, path::PathBuf, sync::Arc};

use sim_kernel::{
    AbiVersion, Callable, CatalogSource, Cx, Error, Export, Lib, LibLoader, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Symbol, Value, Version,
    library::LibSource as KernelLibSource, object::Args,
};

use crate::{
    CliBoot, LibSourceSpec, LoadSession, Payload,
    handoff::{CLI_MAIN_ENTRYPOINT, cli_main_entrypoint_symbol},
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

struct FixtureLib {
    manifest: LibManifest,
    codec: Option<String>,
    entrypoint: Option<(Symbol, FixtureEntrypoint)>,
}

impl FixtureLib {
    fn codec(name: &str, entrypoint: Option<FixtureEntrypoint>) -> Self {
        let entrypoint_symbol = cli_main_entrypoint_symbol(&format!("codec-{name}"));
        let mut exports = vec![codec_export(name)];
        if entrypoint.is_some() {
            exports.push(cli_main_export(entrypoint_symbol.clone()));
        }
        Self {
            manifest: manifest(&format!("codec-{name}"), exports),
            codec: Some(name.to_owned()),
            entrypoint: entrypoint.map(|entrypoint| (entrypoint_symbol, entrypoint)),
        }
    }

    fn app(id: &str, entrypoint: FixtureEntrypoint) -> Self {
        let entrypoint_symbol = cli_main_entrypoint_symbol(id);
        Self {
            manifest: manifest(id, vec![cli_main_export(entrypoint_symbol.clone())]),
            codec: None,
            entrypoint: Some((entrypoint_symbol, entrypoint)),
        }
    }

    fn plain(id: &str) -> Self {
        Self {
            manifest: manifest(id, Vec::new()),
            codec: None,
            entrypoint: None,
        }
    }
}

impl Lib for FixtureLib {
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
        Ok(())
    }
}

struct VerbCatalogLoader;

impl LibLoader for VerbCatalogLoader {
    fn can_load(&self, source: &KernelLibSource) -> bool {
        matches!(source, KernelLibSource::Bytes(_))
    }

    fn load(&self, _cx: &mut Cx, source: KernelLibSource) -> sim_kernel::Result<Box<dyn Lib>> {
        let KernelLibSource::Bytes(bytes) = source else {
            return Err(Error::Lib("verb catalog loader needs bytes".to_owned()));
        };
        let payload = String::from_utf8(bytes)
            .map_err(|err| Error::Lib(format!("verb catalog bytes are not UTF-8: {err}")))?;
        let (verb, expected_args) = catalog_payload(payload);
        let manifest_id = verb.clone();
        let expected_verb: &'static str = Box::leak(verb.clone().into_boxed_str());
        let expected_args = expected_args
            .into_iter()
            .map(|arg| Box::leak(arg.into_boxed_str()) as &'static str)
            .collect();
        Ok(Box::new(FixtureLib::app(
            &manifest_id,
            FixtureEntrypoint::echo(ExpectedEnvelope {
                codec: "codec/test",
                verb: expected_verb,
                args: expected_args,
                eval: "nil",
                script: "nil",
                stdin: "nil",
            }),
        )))
    }
}

fn catalog_payload(payload: String) -> (String, Vec<String>) {
    let mut parts = payload.split('\0').map(str::to_owned).collect::<Vec<_>>();
    let verb = parts.remove(0);
    let args = if parts.is_empty() {
        vec![verb.clone(), "--dry-run".to_owned()]
    } else {
        parts
    };
    (verb, args)
}

fn catalog_bytes(verb: &str, args: &[&str]) -> Vec<u8> {
    std::iter::once(verb)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join("\0")
        .into_bytes()
}

#[derive(Clone)]
struct FixtureEntrypoint {
    mode: EntrypointMode,
}

impl FixtureEntrypoint {
    fn success() -> Self {
        Self {
            mode: EntrypointMode::Return(true),
        }
    }

    fn failure() -> Self {
        Self {
            mode: EntrypointMode::Return(false),
        }
    }

    fn echo(expected: ExpectedEnvelope) -> Self {
        Self {
            mode: EntrypointMode::Echo(expected),
        }
    }
}

#[derive(Clone)]
enum EntrypointMode {
    Return(bool),
    Echo(ExpectedEnvelope),
}

impl Object for FixtureEntrypoint {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok(CLI_MAIN_ENTRYPOINT.to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for FixtureEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for FixtureEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        match &self.mode {
            EntrypointMode::Return(success) => cx.factory().bool(*success),
            EntrypointMode::Echo(expected) => {
                let Some(envelope) = args.values().first() else {
                    return Err(Error::Eval("missing envelope argument".to_owned()));
                };
                let matched = expected.matches(cx, envelope)?;
                cx.factory().bool(matched)
            }
        }
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

#[test]
fn explicit_loaded_lib_entrypoint_wins_over_codec_entrypoint() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("app/success".to_owned())],
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || {
            Box::new(FixtureLib::codec(
                "test",
                Some(FixtureEntrypoint::failure()),
            ))
        })
        .with_host_factory("app/success", || {
            Box::new(FixtureLib::app("app-success", FixtureEntrypoint::success()))
        });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn default_verb_sources_load_when_verb_has_no_explicit_loads() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        payload: Payload {
            args: vec![OsString::from("repl")],
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
        .with_host_factory("app/repl", || {
            Box::new(FixtureLib::app("app-repl", FixtureEntrypoint::success()))
        })
        .with_default_verb_sources("repl", vec![LibSourceSpec::Host("app/repl".to_owned())]);

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(session.receipts().len(), 2);
    assert_eq!(session.receipts()[1].manifest.id, Symbol::new("app-repl"));
}

#[test]
fn explicit_loads_override_default_verb_sources() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("app/explicit".to_owned())],
        payload: Payload {
            args: vec![OsString::from("repl")],
            ..Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
        .with_host_factory("app/default", || {
            Box::new(FixtureLib::app("app-default", FixtureEntrypoint::failure()))
        })
        .with_host_factory("app/explicit", || {
            Box::new(FixtureLib::app(
                "app-explicit",
                FixtureEntrypoint::success(),
            ))
        })
        .with_default_verb_sources("repl", vec![LibSourceSpec::Host("app/default".to_owned())]);

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
    assert_eq!(session.receipts().len(), 2);
    assert_eq!(
        session.receipts()[1].manifest.id,
        Symbol::new("app-explicit")
    );
}

#[test]
fn boot_codec_entrypoint_is_fallback() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        ..CliBoot::default()
    };
    let mut session = LoadSession::new().with_host_factory("codec/test", || {
        Box::new(FixtureLib::codec(
            "test",
            Some(FixtureEntrypoint::success()),
        ))
    });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn false_result_maps_to_failure_exit_code() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("app/fail".to_owned())],
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
        .with_host_factory("app/fail", || {
            Box::new(FixtureLib::app("app-fail", FixtureEntrypoint::failure()))
        });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 1);
}

#[test]
fn entrypoint_receives_cli_envelope_value() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("app/echo".to_owned())],
        native_audio_provider: None,
        list: false,
        inspect: None,
        payload: Payload {
            args: vec![
                OsString::from("run"),
                OsString::from("--port"),
                OsString::from("9"),
            ],
            eval: Some("(+ 1 2)".to_owned()),
            script: Some(PathBuf::from("demo.sim")),
            stdin: Some("input".to_owned()),
        },
    };
    let expected = ExpectedEnvelope {
        codec: "codec/test",
        verb: "run",
        args: vec!["run", "--port", "9"],
        eval: "(+ 1 2)",
        script: "demo.sim",
        stdin: "input",
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
        .with_host_factory("app/echo", move || {
            Box::new(FixtureLib::app(
                "app-echo",
                FixtureEntrypoint::echo(expected.clone()),
            ))
        });

    let code = session.run_loaded_boot(&boot).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn target_verbs_dispatch_from_symbol_loaded_libs_without_baked_cli() {
    for verb in ["server", "atelier", "cookbook", "browse", "agent"] {
        let boot = CliBoot {
            codec: Some("test".to_owned()),
            loads: vec![LibSourceSpec::Symbol(verb.to_owned())],
            payload: Payload {
                args: vec![OsString::from(verb), OsString::from("--dry-run")],
                ..Payload::default()
            },
            ..CliBoot::default()
        };
        let mut session = LoadSession::new()
            .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
            .with_loader(VerbCatalogLoader)
            .with_catalog_source(verb, CatalogSource::Bytes(verb.as_bytes().to_vec()));

        let code = session.run_loaded_boot(&boot).unwrap();

        assert_eq!(code, 0, "{verb} should dispatch to its loaded lib");
    }
}

#[test]
fn documented_command_equivalents_preserve_mode_payloads() {
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "server-start",
            "server",
            &["server", "--port", "4500", "--serve", "agent://planner"],
        ),
        ("server-eval", "server", &["server", "--eval", "(+ 1 2)"]),
        (
            "server-trigger",
            "server",
            &["server", "--trigger", "cron://daily"],
        ),
        (
            "atelier-shell",
            "atelier",
            &[
                "atelier",
                "--control-root",
                ".",
                "--cache",
                ".sim/atelier/shell.json",
            ],
        ),
        ("cookbook-list", "cookbook", &["cookbook", "list"]),
        (
            "cookbook-run",
            "cookbook",
            &["cookbook", "run", "codec/lisp/hello-world"],
        ),
    ];

    for (label, verb, args) in cases {
        let boot = CliBoot {
            codec: Some("test".to_owned()),
            loads: vec![LibSourceSpec::Symbol((*verb).to_owned())],
            payload: Payload {
                args: args.iter().map(OsString::from).collect(),
                ..Payload::default()
            },
            ..CliBoot::default()
        };
        let mut session = LoadSession::new()
            .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
            .with_loader(VerbCatalogLoader)
            .with_catalog_source(*verb, CatalogSource::Bytes(catalog_bytes(verb, args)));

        let code = session.run_loaded_boot(&boot).unwrap();

        assert_eq!(code, 0, "{label} should dispatch through loaded lib");
    }
}

#[test]
fn missing_entrypoint_lists_loaded_libs_and_load_hint() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("app/plain".to_owned())],
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::codec("test", None)))
        .with_host_factory("app/plain", || Box::new(FixtureLib::plain("app-plain")));

    let err = session.run_loaded_boot(&boot).unwrap_err().to_string();

    assert!(err.contains("no loaded lib claims cli/main"));
    assert!(err.contains("codec-test"));
    assert!(err.contains("app-plain"));
    assert!(err.contains("load one with --load"));
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
