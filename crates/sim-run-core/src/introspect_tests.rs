use std::{fs, path::PathBuf, sync::Arc};

use sim_kernel::{
    AbiVersion, Callable, Cx, Error, Export, Lib, LibLoader, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Symbol, Value, Version,
    library::{LibSource as KernelLibSource, LibTarget::HostRegistered},
    object::Args,
};

use crate::{CliBoot, CratesIoResolver, LibSourceSpec, LoadSession, Payload};

fn value_manifest(id: &str, export: &str, target: LibTarget) -> LibManifest {
    manifest(
        id,
        target,
        vec![Export::Value {
            symbol: Symbol::new(export),
        }],
    )
}

fn codec_manifest(id: &str, codec_name: &str, target: LibTarget) -> LibManifest {
    manifest(
        id,
        target,
        vec![Export::Codec {
            symbol: Symbol::qualified("codec", codec_name),
            codec_id: None,
        }],
    )
}

fn delegate_manifest(id: &str, symbol: Symbol) -> LibManifest {
    manifest(
        id,
        HostRegistered,
        vec![Export::Function {
            symbol,
            function_id: None,
        }],
    )
}

fn manifest(id: &str, target: LibTarget, exports: Vec<Export>) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version("0.1.0".to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports,
    }
}

struct ValueLib {
    manifest: LibManifest,
    export: Symbol,
}

impl ValueLib {
    fn new(id: &str, export: &str, target: LibTarget) -> Self {
        Self {
            manifest: value_manifest(id, export, target),
            export: Symbol::new(export),
        }
    }
}

impl Lib for ValueLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        linker.value(self.export.clone(), cx.factory().bool(true).unwrap())
    }
}

struct CodecLib {
    manifest: LibManifest,
    codec: Symbol,
}

impl CodecLib {
    fn new(id: &str, codec_name: &str, target: LibTarget) -> Self {
        Self {
            manifest: codec_manifest(id, codec_name, target),
            codec: Symbol::qualified("codec", codec_name),
        }
    }
}

impl Lib for CodecLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        linker.codec_value(self.codec.clone(), cx.factory().bool(true)?)?;
        Ok(())
    }
}

struct DelegateLib {
    manifest: LibManifest,
    symbol: Symbol,
    output: &'static str,
}

impl DelegateLib {
    fn new(id: &str, symbol: Symbol, output: &'static str) -> Self {
        Self {
            manifest: delegate_manifest(id, symbol.clone()),
            symbol,
            output,
        }
    }
}

impl Lib for DelegateLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        linker.function_value(
            self.symbol.clone(),
            cx.factory().opaque(Arc::new(DelegateFn {
                output: self.output,
            }))?,
        )?;
        Ok(())
    }
}

struct DelegateFn {
    output: &'static str,
}

impl Object for DelegateFn {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("delegate".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for DelegateFn {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for DelegateFn {
    fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<Value> {
        cx.factory().string(self.output.to_owned())
    }
}

struct ArtifactLoader;

impl LibLoader for ArtifactLoader {
    fn can_load(&self, source: &KernelLibSource) -> bool {
        sim_run_loaders::bytes_from_source(source).is_ok_and(|bytes| bytes.is_some())
            || sim_run_loaders::path_from_source(source).is_ok_and(|path| path.is_some())
    }

    fn load(&self, _cx: &mut Cx, source: KernelLibSource) -> sim_kernel::Result<Box<dyn Lib>> {
        let bytes = artifact_bytes(source)?;
        artifact_lib(&bytes)
    }

    fn inspect_manifest(
        &self,
        _cx: &mut Cx,
        source: &KernelLibSource,
    ) -> sim_kernel::Result<Option<LibManifest>> {
        let bytes = artifact_bytes_ref(source)?;
        Ok(Some(artifact_manifest(&bytes)?))
    }
}

#[test]
fn list_output_includes_catalog_loaded_receipts_and_crates_artifacts() {
    let path = temp_artifact("list-path");
    let crate_artifact = temp_artifact("list-crate");
    let cache = temp_cache("list-cache");
    fs::write(&path, b"path-lib").unwrap();
    fs::write(&crate_artifact, b"crate-lib").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-lib-crate",
        "0.1.0",
        crate_artifact.clone(),
    );
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![
            LibSourceSpec::Host("host/demo".to_owned()),
            LibSourceSpec::Bytes(b"bytes-lib".to_vec()),
            LibSourceSpec::Path(path.clone()),
            LibSourceSpec::Symbol("catalog/demo".to_owned()),
            LibSourceSpec::CratesIo("sim-lib-crate@0.1.0".parse().unwrap()),
        ],
        native_audio_provider: None,
        config: crate::ConfigLoadOptions::default(),
        list: true,
        inspect: None,
        config_report: None,
        payload: Payload::default(),
    };
    let mut session = session()
        .with_crates_io_resolver(resolver)
        .with_host_factory("host/demo", || {
            Box::new(ValueLib::new("host-demo", "host-value", HostRegistered))
        });
    session.add_catalog_source(
        "catalog/demo",
        sim_run_loaders::catalog_bytes_source(b"catalog-lib".to_vec()),
    );

    let output = session.run_loaded_introspection(&boot).unwrap();

    assert!(output.contains("catalog sources:\n- symbol:catalog/demo -> bytes:11 bytes"));
    assert!(output.contains("crates.io:sim-lib-crate@0.1.0 source=registry"));
    assert!(output.contains("loaded libs:"));
    for id in [
        "codec-test",
        "host-demo",
        "bytes-lib",
        "path-lib",
        "catalog-lib",
        "crate-lib",
    ] {
        assert!(output.contains(&format!("lib={id}")), "{output}");
    }
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(crate_artifact);
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn inspect_loaded_lib_and_export_uses_open_export_records() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("host/demo".to_owned())],
        native_audio_provider: None,
        config: crate::ConfigLoadOptions::default(),
        list: false,
        inspect: Some("host-demo".to_owned()),
        config_report: None,
        payload: Payload::default(),
    };
    let mut lib_session = session().with_host_factory("host/demo", || {
        Box::new(ValueLib::new("host-demo", "host-value", HostRegistered))
    });

    let lib_output = lib_session.run_loaded_introspection(&boot).unwrap();

    assert!(lib_output.contains("lib host-demo"));
    assert!(lib_output.contains("requested host:host/demo"));
    assert!(lib_output.contains("kind=value symbol=host-value state=resolved"));

    let export_boot = CliBoot {
        inspect: Some("host-value".to_owned()),
        ..boot
    };
    let mut export_session = session().with_host_factory("host/demo", || {
        Box::new(ValueLib::new("host-demo", "host-value", HostRegistered))
    });

    let export_output = export_session
        .run_loaded_introspection(&export_boot)
        .unwrap();

    assert!(export_output.contains("export host-value"));
    assert!(export_output.contains("lib=host-demo kind=value symbol=host-value state=resolved"));
}

#[test]
fn inspect_host_registered_tty_surface_reports_loadable_functions() {
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![LibSourceSpec::Host("surface/tty".to_owned())],
        native_audio_provider: None,
        config: crate::ConfigLoadOptions::default(),
        list: false,
        inspect: Some("view/tty".to_owned()),
        config_report: None,
        payload: Payload::default(),
    };
    let mut session =
        session().with_host_factory("surface/tty", || Box::new(sim_view_tty::TtyViewLib::new()));

    let output = session.run_loaded_introspection(&boot).unwrap();

    assert!(output.contains("lib view/tty"), "{output}");
    assert!(
        output.contains("kind=function symbol=surface/tty/render state=resolved"),
        "{output}"
    );
    assert!(
        output.contains("kind=function symbol=surface/tty/intent state=resolved"),
        "{output}"
    );
}

#[test]
fn inspect_source_handles_host_bytes_path_symbol_and_crates_sources() {
    let path = temp_artifact("inspect-path");
    let crate_artifact = temp_artifact("inspect-crate");
    let cache = temp_cache("inspect-cache");
    fs::write(&path, b"path-lib").unwrap();
    fs::write(&crate_artifact, b"crate-lib").unwrap();

    let cases = vec![
        ("host:host/demo".to_owned(), "lib host-demo"),
        ("bytes:bytes-lib".to_owned(), "lib bytes-lib"),
        (format!("path:{}", path.display()), "lib path-lib"),
        ("symbol:catalog/demo".to_owned(), "lib catalog-lib"),
        ("crates.io:sim-lib-crate@0.1.0".to_owned(), "lib crate-lib"),
    ];

    for (target, expected) in cases {
        let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
            "sim-lib-crate",
            "0.1.0",
            crate_artifact.clone(),
        );
        let mut session = session()
            .with_crates_io_resolver(resolver)
            .with_host_factory("host/demo", || {
                Box::new(ValueLib::new("host-demo", "host-value", HostRegistered))
            });
        session.add_catalog_source(
            "catalog/demo",
            sim_run_loaders::catalog_bytes_source(b"catalog-lib".to_vec()),
        );
        let boot = CliBoot {
            codec: Some("test".to_owned()),
            list: false,
            inspect: Some(target),
            ..CliBoot::default()
        };

        let output = session.run_loaded_introspection(&boot).unwrap();

        assert!(output.contains(expected), "{output}");
        assert!(output.contains("exports:\n- kind=value"), "{output}");
    }

    let _ = fs::remove_file(path);
    let _ = fs::remove_file(crate_artifact);
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn loaded_delegate_can_own_list_and_inspect_output() {
    let list_symbol = Symbol::qualified("cli", "list");
    let inspect_symbol = Symbol::qualified("cli", "inspect");
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![
            LibSourceSpec::Host("delegate/list".to_owned()),
            LibSourceSpec::Host("delegate/inspect".to_owned()),
        ],
        native_audio_provider: None,
        config: crate::ConfigLoadOptions::default(),
        list: true,
        inspect: Some("demo".to_owned()),
        config_report: None,
        payload: Payload::default(),
    };
    let mut session = session()
        .with_host_factory("delegate/list", move || {
            Box::new(DelegateLib::new(
                "delegate-list",
                list_symbol.clone(),
                "delegated list",
            ))
        })
        .with_host_factory("delegate/inspect", move || {
            Box::new(DelegateLib::new(
                "delegate-inspect",
                inspect_symbol.clone(),
                "delegated inspect",
            ))
        });

    let output = session.run_loaded_introspection(&boot).unwrap();

    assert!(output.contains("delegated list"));
    assert!(output.contains("delegated inspect"));
    assert!(!output.contains("catalog sources:"));
}

fn session() -> LoadSession {
    LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_host_factory("codec/test", || {
            Box::new(CodecLib::new("codec-test", "test", HostRegistered))
        })
}

fn artifact_lib(bytes: &[u8]) -> sim_kernel::Result<Box<dyn Lib>> {
    match bytes {
        b"bytes-lib" => Ok(Box::new(ValueLib::new(
            "bytes-lib",
            "bytes-value",
            LibTarget::DataOnly,
        ))),
        b"path-lib" => Ok(Box::new(ValueLib::new(
            "path-lib",
            "path-value",
            LibTarget::DataOnly,
        ))),
        b"catalog-lib" => Ok(Box::new(ValueLib::new(
            "catalog-lib",
            "catalog-value",
            LibTarget::DataOnly,
        ))),
        b"crate-lib" => Ok(Box::new(ValueLib::new(
            "crate-lib",
            "crate-value",
            LibTarget::DataOnly,
        ))),
        _ => Err(Error::Lib("artifact rejected".to_owned())),
    }
}

fn artifact_manifest(bytes: &[u8]) -> sim_kernel::Result<LibManifest> {
    match bytes {
        b"bytes-lib" => Ok(value_manifest(
            "bytes-lib",
            "bytes-value",
            LibTarget::DataOnly,
        )),
        b"path-lib" => Ok(value_manifest(
            "path-lib",
            "path-value",
            LibTarget::DataOnly,
        )),
        b"catalog-lib" => Ok(value_manifest(
            "catalog-lib",
            "catalog-value",
            LibTarget::DataOnly,
        )),
        b"crate-lib" => Ok(value_manifest(
            "crate-lib",
            "crate-value",
            LibTarget::DataOnly,
        )),
        _ => Err(Error::Lib("artifact rejected".to_owned())),
    }
}

fn artifact_bytes(source: KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    if let Some(bytes) = sim_run_loaders::bytes_from_source(&source)? {
        return Ok(bytes);
    }
    if let Some(path) = sim_run_loaders::path_from_source(&source)? {
        return fs::read(path).map_err(|err| Error::Lib(format!("read artifact: {err}")));
    }
    Err(Error::Lib("unsupported fixture source".to_owned()))
}

fn artifact_bytes_ref(source: &KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    if let Some(bytes) = sim_run_loaders::bytes_from_source(source)? {
        return Ok(bytes);
    }
    if let Some(path) = sim_run_loaders::path_from_source(source)? {
        return fs::read(path).map_err(|err| Error::Lib(format!("read artifact: {err}")));
    }
    Err(Error::Lib("unsupported fixture source".to_owned()))
}

fn temp_artifact(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-introspect-{}-{label}.artifact",
        std::process::id()
    ))
}

fn temp_cache(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-introspect-cache-{}-{label}",
        std::process::id()
    ))
}
