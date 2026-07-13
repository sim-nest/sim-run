use std::{
    fs,
    path::{Path, PathBuf},
};

use sim_config::{
    ConfigDir, ConfigLayer, ConfigProbeReport, ConfigProbeStatus, ConfigRoots, ConfigSecretField,
    ConfigSource, ConfigTable, ProbeMode,
};
use sim_kernel::{AbiVersion, Expr, Lib, LibManifest, LibTarget, Linker, LoadCx, Symbol, Version};

use crate::{
    CliBoot, ConfigLoadOptions, ConfigReportKind, ConfigReportRequest, LibSourceSpec, LoadSession,
    LoadedStateReport, RuntimeConfigState, format_config_status, format_config_status_json,
    format_effective_config, format_effective_config_json, load_config_sources,
};

fn lib(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn temp_root(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-run-config-report-{}-{label}-{nanos}",
        std::process::id()
    ))
}

fn write_file(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("test path has parent")).unwrap();
    fs::write(path, contents).unwrap();
}

fn roots(home: &Path, work: &Path) -> ConfigLoadOptions {
    ConfigLoadOptions::with_roots(ConfigRoots::new(
        Some(home.to_path_buf()),
        work.to_path_buf(),
    ))
}

#[test]
fn config_status_reports_loaded_libs_and_source_provenance() {
    let base = temp_root("status");
    let home = base.join("home");
    let work = base.join("work");
    write_file(
        &home.join("libs").join("test").join("demo.toml"),
        r#"mode = "home"
"#,
    );
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("test/demo".to_owned())],
        config: roots(&home, &work),
        config_report: Some(ConfigReportRequest {
            kind: ConfigReportKind::Status,
            json: false,
        }),
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/test", || Box::new(FixtureLib::new("codec/test")))
        .with_host_factory("test/demo", || Box::new(FixtureLib::new("test/demo")));

    let output = session.run_config_report(&boot).unwrap();

    assert!(
        output.contains("role=boot-codec lib=codec/test"),
        "{output}"
    );
    assert!(output.contains("role=library lib=test/demo"), "{output}");
    assert!(output.contains("source=home-file:"), "{output}");
    assert!(output.contains("status=found"), "{output}");
    assert!(output.contains("status=missing"), "{output}");

    let _ = fs::remove_dir_all(base);
}

#[test]
fn effective_config_json_matches_stable_shape() {
    let base = temp_root("effective-json");
    let config_file = base.join("sim.toml");
    write_file(
        &config_file,
        r#"[sim/cookbook]
minimum_loaded = ["codec/lisp"]
"#,
    );
    let boot = CliBoot {
        config: ConfigLoadOptions {
            roots: ConfigRoots::new(None, base.join("work")),
            read_files: true,
            single_file: Some(config_file),
            site_sources: Vec::new(),
        },
        config_report: Some(ConfigReportRequest {
            kind: ConfigReportKind::Effective {
                lib: lib("sim", "cookbook"),
            },
            json: true,
        }),
        ..CliBoot::default()
    };
    let mut session = LoadSession::new();

    let output = session.run_config_report(&boot).unwrap();

    assert_eq!(
        output,
        "{\"lib\":\"sim/cookbook\",\"table\":{\"minimum_loaded\":[\"codec/lisp\"]}}\n"
    );

    let _ = fs::remove_dir_all(base);
}

#[test]
fn missing_explicit_source_reports_diagnostic() {
    let base = temp_root("missing-source");
    let missing = base.join("missing.toml");
    let mut cx = sim_kernel::Cx::new(
        std::sync::Arc::new(sim_kernel::NoopEvalPolicy),
        std::sync::Arc::new(sim_kernel::DefaultFactory),
    );
    let state = load_config_sources(
        &mut cx,
        &ConfigLoadOptions {
            roots: ConfigRoots::new(None, base.join("work")),
            read_files: true,
            single_file: Some(missing.clone()),
            site_sources: Vec::new(),
        },
        &[lib("codec", "lisp")],
    );

    assert!(state.source_reports().iter().any(
        |report| matches!(report.source, ConfigSource::SingleFile { ref path } if path == &missing)
            && report.status == crate::SourceStatus::Missing
    ));
    assert_eq!(
        state.diagnostics(),
        &[format!("config file not found: {}", missing.display())]
    );

    let _ = fs::remove_dir_all(base);
}

#[test]
fn text_and_json_renderers_redact_secret_fields() {
    let model = lib("model", "defaults");
    let table = ConfigTable::new(
        model.clone(),
        Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("provider")),
                Expr::String("openai".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("api_key")),
                Expr::String("raw-secret-key".to_owned()),
            ),
        ]),
    )
    .unwrap();
    let mut state = RuntimeConfigState::default();
    state.push_layer(ConfigLayer::new(
        ConfigSource::Explicit {
            label: "test".to_owned(),
        },
        ConfigDir {
            entries: vec![table],
        },
    ));
    state.extend_secret_fields([ConfigSecretField {
        lib: model.clone(),
        key: "api_key".to_owned(),
    }]);
    let mut session = LoadSession::new();
    *session.config_state_mut() = state;
    let report = LoadedStateReport::from_session(&session);

    let text = format_effective_config(&report, &model);
    let json = format_effective_config_json(&report, &model);
    let status_json = format_config_status_json(&report);

    for rendered in [&text, &json, &status_json] {
        assert!(!rendered.contains("raw-secret-key"), "{rendered}");
        assert!(rendered.contains("[redacted]"), "{rendered}");
    }
}

#[test]
fn config_status_renders_typed_probe_report_records() {
    let lib = lib("stream", "host");
    let mut state = RuntimeConfigState::default();
    state.push_probe_report(ConfigProbeReport {
        probe: Symbol::qualified("config", "fake"),
        lib: lib.clone(),
        mode: ProbeMode::Modeled,
        status: ConfigProbeStatus::Applied,
        emitted_keys: vec!["backend".to_owned()],
    });
    state.push_probe_report(ConfigProbeReport {
        probe: Symbol::qualified("config", "real"),
        lib,
        mode: ProbeMode::Real,
        status: ConfigProbeStatus::Denied {
            capability: "hardware_inventory".to_owned(),
        },
        emitted_keys: Vec::new(),
    });
    let mut session = LoadSession::new();
    *session.config_state_mut() = state;
    let report = LoadedStateReport::from_session(&session);

    let text = format_config_status(&report);
    let json = format_config_status_json(&report);

    assert!(
        text.contains(
            "probe=config/fake lib=stream/host mode=modeled status=applied emitted=backend"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "probe=config/real lib=stream/host mode=real status=denied capability=hardware_inventory emitted=-"
        ),
        "{text}"
    );
    assert!(
        json.contains(
            "\"probe\":\"config/fake\",\"lib\":\"stream/host\",\"mode\":\"modeled\",\"status\":\"applied\",\"emitted_keys\":[\"backend\"]"
        ),
        "{json}"
    );
    assert!(
        json.contains(
            "\"probe\":\"config/real\",\"lib\":\"stream/host\",\"mode\":\"real\",\"status\":\"denied\",\"capability\":\"hardware_inventory\",\"emitted_keys\":[]"
        ),
        "{json}"
    );
}

struct FixtureLib {
    id: Symbol,
}

impl FixtureLib {
    fn new(id: &str) -> Self {
        Self {
            id: crate::source::symbol_from_text(id),
        }
    }
}

impl Lib for FixtureLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.id.clone(),
            version: Version("0.1.0".to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: Vec::new(),
        }
    }

    fn load(&self, _cx: &mut LoadCx, _linker: &mut Linker) -> sim_kernel::Result<()> {
        Ok(())
    }
}
