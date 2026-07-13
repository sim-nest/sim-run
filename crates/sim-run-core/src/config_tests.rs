use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use sim_config::{
    ConfigDir, ConfigLayer, ConfigProbe, ConfigProbeCaps, ConfigProbeReport, ConfigProbeRequest,
    ConfigProbeStatus, ConfigRoots, ConfigSource, ConfigView, ProbeMode,
};
use sim_kernel::{
    AbiVersion, Cx, DefaultFactory, Expr, Lib, LibManifest, LibTarget, Linker, LoadCx,
    NoopEvalPolicy, Symbol, Version,
};

use crate::{
    CliBoot, ConfigLoadOptions, LibSourceSpec, LoadSession, load_config_sources,
    load_config_sources_with_probes, run_config_probe,
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
        "sim-run-config-{}-{label}-{nanos}",
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

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

struct ModeledDefaultsProbe;

impl ModeledDefaultsProbe {
    fn symbol() -> Symbol {
        Symbol::qualified("config", "fake")
    }
}

impl ConfigProbe for ModeledDefaultsProbe {
    fn symbol(&self) -> Symbol {
        Self::symbol()
    }

    fn probe(&self, request: &ConfigProbeRequest) -> (Option<ConfigLayer>, ConfigProbeReport) {
        let table = Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("backend")),
                Expr::String("modeled".to_owned()),
            ),
            (
                Expr::Symbol(Symbol::new("probe_only")),
                Expr::String("yes".to_owned()),
            ),
        ]);
        let layer = ConfigLayer::new(
            ConfigSource::Probe {
                probe: self.symbol(),
                mode: request.mode,
            },
            ConfigDir::one(request.lib.clone(), table).unwrap(),
        );
        (
            Some(layer),
            ConfigProbeReport {
                probe: self.symbol(),
                lib: request.lib.clone(),
                mode: request.mode,
                status: ConfigProbeStatus::Applied,
                emitted_keys: vec!["backend".to_owned(), "probe_only".to_owned()],
            },
        )
    }
}

struct HardwareProbe;

impl HardwareProbe {
    fn symbol() -> Symbol {
        Symbol::qualified("config", "hardware")
    }
}

impl ConfigProbe for HardwareProbe {
    fn symbol(&self) -> Symbol {
        Self::symbol()
    }

    fn probe(&self, request: &ConfigProbeRequest) -> (Option<ConfigLayer>, ConfigProbeReport) {
        if request.mode == ProbeMode::Real && !request.caps.hardware_inventory {
            return (
                None,
                ConfigProbeReport {
                    probe: self.symbol(),
                    lib: request.lib.clone(),
                    mode: request.mode,
                    status: ConfigProbeStatus::Denied {
                        capability: "hardware_inventory".to_owned(),
                    },
                    emitted_keys: Vec::new(),
                },
            );
        }
        let layer = ConfigLayer::new(
            ConfigSource::Probe {
                probe: self.symbol(),
                mode: request.mode,
            },
            ConfigDir::one(
                request.lib.clone(),
                Expr::Map(vec![(
                    Expr::Symbol(Symbol::new("backend")),
                    Expr::String("real".to_owned()),
                )]),
            )
            .unwrap(),
        );
        (
            Some(layer),
            ConfigProbeReport {
                probe: self.symbol(),
                lib: request.lib.clone(),
                mode: request.mode,
                status: ConfigProbeStatus::Applied,
                emitted_keys: vec!["backend".to_owned()],
            },
        )
    }
}

struct FailingProbe;

impl ConfigProbe for FailingProbe {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("config", "failing")
    }

    fn probe(&self, request: &ConfigProbeRequest) -> (Option<ConfigLayer>, ConfigProbeReport) {
        (
            None,
            ConfigProbeReport {
                probe: self.symbol(),
                lib: request.lib.clone(),
                mode: request.mode,
                status: ConfigProbeStatus::Failed {
                    message: "probe fixture failed".to_owned(),
                },
                emitted_keys: Vec::new(),
            },
        )
    }
}

#[test]
fn home_and_work_config_roots_load_in_documented_order() {
    let base = temp_root("home-work-order");
    let home = base.join("home");
    let work = base.join("work");
    let cookbook = lib("sim", "cookbook");
    write_file(
        &home.join("libs").join("sim").join("cookbook.toml"),
        r#"mode = "home"
keep = "home"
"#,
    );
    write_file(
        &work.join("libs").join("sim").join("cookbook.toml"),
        r#"mode = "work"
"#,
    );

    let mut cx = test_cx();
    let state = load_config_sources(
        &mut cx,
        &roots(&home, &work),
        std::slice::from_ref(&cookbook),
    );

    assert!(state.diagnostics().is_empty(), "{:?}", state.diagnostics());
    assert_eq!(state.layers().len(), 2);
    assert!(matches!(
        state.layers()[0].source,
        ConfigSource::HomeFile { .. }
    ));
    assert!(matches!(
        state.layers()[1].source,
        ConfigSource::WorkFile { .. }
    ));
    let table = state.effective().dir.table(&cookbook).unwrap();
    let view = ConfigView::new(table);
    assert_eq!(view.string("mode"), Some("work"));
    assert_eq!(view.string("keep"), Some("home"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn modeled_probe_defaults_load_before_home_and_work_files() {
    let base = temp_root("probe-order");
    let home = base.join("home");
    let work = base.join("work");
    let stream_host = lib("stream", "host");
    write_file(
        &home.join("libs").join("stream").join("host.toml"),
        r#"backend = "home"
"#,
    );
    write_file(
        &work.join("libs").join("stream").join("host.toml"),
        r#"backend = "work"
"#,
    );
    let mut cx = test_cx();
    let probe = ModeledDefaultsProbe;
    let probes: [&dyn ConfigProbe; 1] = [&probe];

    let state = load_config_sources_with_probes(
        &mut cx,
        &roots(&home, &work),
        std::slice::from_ref(&stream_host),
        &probes,
    );

    assert!(state.diagnostics().is_empty(), "{:?}", state.diagnostics());
    assert_eq!(state.layers().len(), 3);
    assert!(matches!(
        state.layers()[0].source,
        ConfigSource::Probe {
            probe: ref layer_probe,
            mode: ProbeMode::Modeled
        } if layer_probe == &ModeledDefaultsProbe::symbol()
    ));
    assert!(matches!(
        state.layers()[1].source,
        ConfigSource::HomeFile { .. }
    ));
    assert!(matches!(
        state.layers()[2].source,
        ConfigSource::WorkFile { .. }
    ));
    assert_eq!(state.probe_reports().len(), 1);
    assert_eq!(state.probe_reports()[0].status, ConfigProbeStatus::Applied);
    assert_eq!(
        state.probe_reports()[0].emitted_keys,
        ["backend", "probe_only"]
    );
    let table = state.effective().dir.table(&stream_host).unwrap();
    let view = ConfigView::new(table);
    assert_eq!(view.string("backend"), Some("work"));
    assert_eq!(view.string("probe_only"), Some("yes"));
    let probe_trace = state
        .effective()
        .trace
        .iter()
        .find(|trace| trace.key == "probe_only")
        .unwrap();
    assert!(matches!(
        probe_trace.source,
        ConfigSource::Probe {
            probe: ref layer_probe,
            mode: ProbeMode::Modeled
        } if layer_probe == &ModeledDefaultsProbe::symbol()
    ));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn real_probe_denial_and_failure_are_non_fatal_reports() {
    let stream_host = lib("stream", "host");
    let hardware = HardwareProbe;
    let failing = FailingProbe;
    let mut state = crate::RuntimeConfigState::default();
    let denied = ConfigProbeRequest {
        lib: stream_host.clone(),
        mode: ProbeMode::Real,
        caps: ConfigProbeCaps::default(),
    };

    run_config_probe(&mut state, &hardware, &denied);

    assert!(state.layers().is_empty());
    assert!(state.diagnostics().is_empty());
    assert_eq!(
        state.probe_reports()[0].status,
        ConfigProbeStatus::Denied {
            capability: "hardware_inventory".to_owned()
        }
    );

    let granted_caps = ConfigProbeCaps {
        hardware_inventory: true,
        ..ConfigProbeCaps::default()
    };
    let granted = ConfigProbeRequest {
        caps: granted_caps,
        ..denied.clone()
    };
    run_config_probe(&mut state, &hardware, &granted);
    run_config_probe(&mut state, &failing, &granted);

    assert_eq!(state.layers().len(), 1);
    assert_eq!(state.probe_reports()[1].status, ConfigProbeStatus::Applied);
    assert_eq!(
        state.probe_reports()[2].status,
        ConfigProbeStatus::Failed {
            message: "probe fixture failed".to_owned()
        }
    );
    assert!(state.diagnostics().is_empty());
}

#[test]
fn per_lib_file_and_single_file_layouts_produce_same_effective_dir() {
    let base = temp_root("layout-equivalence");
    let home = base.join("home");
    let work = base.join("work");
    let single = base.join("sim.toml");
    let cookbook = lib("sim", "cookbook");
    let per_lib = r#"minimum_loaded = ["codec/lisp"]

[[loadable_lib]]
id = "numbers/cas"
source = "symbol:numbers/cas"
"#;
    write_file(
        &home.join("libs").join("sim").join("cookbook.toml"),
        per_lib,
    );
    write_file(
        &single,
        r#"[sim/cookbook]
minimum_loaded = ["codec/lisp"]

[[sim/cookbook.loadable_lib]]
id = "numbers/cas"
source = "symbol:numbers/cas"
"#,
    );

    let mut cx = test_cx();
    let per_lib_state = load_config_sources(
        &mut cx,
        &roots(&home, &work),
        std::slice::from_ref(&cookbook),
    );
    let single_opts = ConfigLoadOptions {
        roots: ConfigRoots::new(None, base.join("unused")),
        read_files: true,
        single_file: Some(single),
        site_sources: Vec::new(),
    };
    let single_state = load_config_sources(&mut cx, &single_opts, &[]);

    assert_eq!(per_lib_state.effective().dir, single_state.effective().dir);

    let _ = fs::remove_dir_all(base);
}

#[test]
fn site_backed_dir_resolves_through_registry_site_export() {
    let mut cx = test_cx();
    let cookbook = lib("sim", "cookbook");
    let site = lib("config", "runtime");
    let dir_expr = Expr::Map(vec![(
        Expr::Symbol(cookbook.clone()),
        Expr::Map(vec![(
            Expr::Symbol(Symbol::new("mode")),
            Expr::String("site".to_owned()),
        )]),
    )]);
    let site_value = cx.factory().expr(dir_expr).unwrap();
    cx.registry_mut()
        .register_site_value(site.clone(), site_value)
        .unwrap();
    let opts = ConfigLoadOptions {
        roots: ConfigRoots::new(None, temp_root("site-unused")),
        read_files: false,
        single_file: None,
        site_sources: vec![site.clone()],
    };

    let state = load_config_sources(&mut cx, &opts, &[]);

    assert!(state.diagnostics().is_empty(), "{:?}", state.diagnostics());
    assert!(matches!(
        state.layers().first().unwrap().source,
        ConfigSource::Site { site: ref layer_site } if layer_site == &site
    ));
    let table = state.effective().dir.table(&cookbook).unwrap();
    assert_eq!(ConfigView::new(table).string("mode"), Some("site"));
}

#[test]
fn load_boot_discovers_codec_and_explicit_host_lib_configs() {
    let base = temp_root("boot-discovers-libs");
    let home = base.join("home");
    let work = base.join("work");
    write_file(
        &home.join("libs").join("codec").join("lisp.toml"),
        r#"mode = "codec"
"#,
    );
    write_file(
        &home.join("libs").join("test").join("demo.toml"),
        r#"mode = "host"
"#,
    );
    let boot = CliBoot {
        loads: vec![LibSourceSpec::Host("test/demo".to_owned())],
        config: roots(&home, &work),
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/lisp", || Box::new(FixtureLib::new("codec/lisp")))
        .with_host_factory("test/demo", || Box::new(FixtureLib::new("test/demo")));

    session.load_boot(&boot).unwrap();

    assert!(
        session
            .config_state()
            .effective()
            .dir
            .table(&lib("codec", "lisp"))
            .is_some()
    );
    assert!(
        session
            .config_state()
            .effective()
            .dir
            .table(&lib("test", "demo"))
            .is_some()
    );

    let _ = fs::remove_dir_all(base);
}

#[test]
fn config_discovery_does_not_load_libs_mentioned_by_config() {
    let base = temp_root("mentioned-lib-not-loaded");
    let single = base.join("sim.toml");
    write_file(
        &single,
        r#"[sim/cookbook]

[[sim/cookbook.loadable_lib]]
id = "numbers/cas"
source = "symbol:numbers/cas"
"#,
    );
    let boot = CliBoot {
        config: ConfigLoadOptions {
            roots: ConfigRoots::new(None, base.join("unused")),
            read_files: true,
            single_file: Some(single),
            site_sources: Vec::new(),
        },
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/lisp", || Box::new(FixtureLib::new("codec/lisp")));

    session.load_boot(&boot).unwrap();

    assert!(
        session
            .config_state()
            .effective()
            .dir
            .table(&lib("sim", "cookbook"))
            .is_some()
    );
    assert!(
        session
            .cx()
            .registry()
            .manifest_by_symbol(&lib("numbers", "cas"))
            .is_none()
    );

    let _ = fs::remove_dir_all(base);
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
