use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use sim_config::{ConfigRoots, ConfigSource};
use sim_kernel::{
    AbiVersion, Cx, DefaultFactory, Expr, Lib, LibManifest, LibTarget, Linker, LoadCx,
    NoopEvalPolicy, Symbol, Version,
};

use crate::{CliBoot, ConfigLoadOptions, LibSourceSpec, LoadSession, load_config_sources};

fn lib(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn table_field<'a>(table: &'a Expr, key: &str) -> Option<&'a Expr> {
    let Expr::Map(entries) = table else {
        return None;
    };
    entries
        .iter()
        .find_map(|(candidate, value)| match candidate {
            Expr::Symbol(symbol) if symbol.as_qualified_str() == key => Some(value),
            Expr::String(text) if text == key => Some(value),
            _ => None,
        })
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
    let table = &state.effective().dir.table(&cookbook).unwrap().table;
    assert_eq!(
        table_field(table, "mode"),
        Some(&Expr::String("work".to_owned()))
    );
    assert_eq!(
        table_field(table, "keep"),
        Some(&Expr::String("home".to_owned()))
    );

    let _ = fs::remove_dir_all(base);
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
    let table = &state.effective().dir.table(&cookbook).unwrap().table;
    assert_eq!(
        table_field(table, "mode"),
        Some(&Expr::String("site".to_owned()))
    );
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
