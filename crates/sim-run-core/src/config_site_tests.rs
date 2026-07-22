use std::sync::Arc;

use sim_config::{ConfigDir, ConfigRoots, ConfigSource, ConfigView};
use sim_kernel::{
    AbiVersion, Args, Callable, Cx, DefaultFactory, Error, Export, Expr, Lib, LibManifest,
    LibTarget, Linker, LoadCx, NoopEvalPolicy, Object, ObjectCompat, Result, Symbol, Value,
    Version,
};

use crate::{
    CliBoot, ConfigLoadOptions, LibSourceSpec, LoadSession, SourceStatus, load_config_sources,
};

fn lib(namespace: &str, name: &str) -> Symbol {
    Symbol::qualified(namespace, name)
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn site_config_dir_expr(target: &Symbol) -> Expr {
    ConfigDir::one(
        target.clone(),
        Expr::Map(vec![(
            Expr::Symbol(Symbol::new("mode")),
            Expr::String("site".to_owned()),
        )]),
    )
    .unwrap()
    .to_expr()
}

#[derive(Clone)]
struct ConfigDirSite {
    site: Symbol,
    dir: Expr,
}

impl ConfigDirSite {
    fn new(site: Symbol, dir: Expr) -> Self {
        Self { site, dir }
    }
}

impl Object for ConfigDirSite {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<config-site {}>", self.site))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ConfigDirSite {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.site.clone()))
    }
}

impl Callable for ConfigDirSite {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let [request] = args.values() else {
            return Err(Error::Eval("config site expects one request".to_owned()));
        };
        let request = request.object().as_expr(cx)?;
        if request != Expr::Symbol(Symbol::qualified("config", "dir")) {
            return Err(Error::Eval(format!(
                "unsupported config site request {request:?}"
            )));
        }
        cx.factory().expr(self.dir.clone())
    }
}

struct BasicLib {
    id: Symbol,
}

impl BasicLib {
    fn new(id: &str) -> Self {
        Self {
            id: crate::source::symbol_from_text(id),
        }
    }
}

impl Lib for BasicLib {
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

    fn load(&self, _cx: &mut LoadCx, _linker: &mut Linker<'_>) -> Result<()> {
        Ok(())
    }
}

struct ConfigSiteLib {
    id: Symbol,
    site: Symbol,
    dir: Expr,
}

impl ConfigSiteLib {
    fn new(site: Symbol, dir: Expr) -> Self {
        Self {
            id: lib("config", "site-lib"),
            site,
            dir,
        }
    }
}

impl Lib for ConfigSiteLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.id.clone(),
            version: Version("0.1.0".to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Site {
                symbol: self.site.clone(),
                runtime_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let site = cx.factory().opaque(Arc::new(ConfigDirSite::new(
            self.site.clone(),
            self.dir.clone(),
        )))?;
        linker.site_value(self.site.clone(), site).map(|_| ())
    }
}

#[test]
fn site_backed_dir_calls_config_dir_operation() {
    let mut cx = test_cx();
    let cookbook = lib("sim", "cookbook");
    let site = lib("config", "runtime");
    let site_value = cx
        .factory()
        .opaque(Arc::new(ConfigDirSite::new(
            site.clone(),
            site_config_dir_expr(&cookbook),
        )))
        .unwrap();
    cx.registry_mut()
        .register_site_value(site.clone(), site_value)
        .unwrap();
    let opts = ConfigLoadOptions {
        roots: ConfigRoots::new(None, std::env::temp_dir().join("sim-run-site-unused")),
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
fn config_site_loaded_by_boot_source_is_visible_before_config_factory() {
    let app = lib("app", "serve");
    let site = lib("config", "runtime");
    let site_for_factory = site.clone();
    let app_for_factory = app.clone();
    let boot = CliBoot {
        config: ConfigLoadOptions {
            roots: ConfigRoots::new(None, std::env::temp_dir().join("sim-run-site-boot-unused")),
            read_files: false,
            single_file: None,
            site_sources: vec![site.clone()],
        },
        payload: crate::Payload {
            args: vec!["serve".into()],
            ..crate::Payload::default()
        },
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_host_factory("codec/lisp", || Box::new(BasicLib::new("codec/lisp")))
        .with_host_factory("config/site", move || {
            Box::new(ConfigSiteLib::new(
                site_for_factory.clone(),
                site_config_dir_expr(&app_for_factory),
            ))
        })
        .with_host_factory_with_config("app/serve", |config| {
            let id = if config.effective().dir.table(&lib("app", "serve")).is_some() {
                "app/configured"
            } else {
                "app/unconfigured"
            };
            Box::new(BasicLib::new(id))
        })
        .with_default_verb_sources(
            "serve",
            vec![
                LibSourceSpec::Host("config/site".to_owned()),
                LibSourceSpec::Host("app/serve".to_owned()),
            ],
        );

    session.load_boot(&boot).unwrap();

    assert!(session.receipts().iter().any(|receipt| {
        receipt.requested_source == LibSourceSpec::Host("app/serve".to_owned())
            && receipt.manifest.id == lib("app", "configured")
    }));
    assert!(session.config_state().source_reports().iter().any(|row| {
        matches!(row.source, ConfigSource::Site { ref site } if site == &lib("config", "runtime"))
            && row.status == SourceStatus::Found
    }));
    let table = session.config_state().effective().dir.table(&app).unwrap();
    assert_eq!(ConfigView::new(table).string("mode"), Some("site"));
}
