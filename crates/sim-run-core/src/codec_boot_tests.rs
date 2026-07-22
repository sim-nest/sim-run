use std::{fs, path::PathBuf};

use sim_kernel::{
    AbiVersion, Cx, Error as KernelError, Export, Lib, LibLoader, LibManifest, LibTarget, Linker,
    LoadCx, Symbol, Version,
    library::{LibSource as KernelLibSource, LibTarget::HostRegistered},
};

use crate::{CliBoot, CratesIoResolver, LibSourceSpec, LoadReceiptRole, LoadSession, Payload};

fn codec_manifest(id: &str, codec_name: &str, target: LibTarget) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version("0.1.0".to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: vec![Export::Codec {
            symbol: Symbol::qualified("codec", codec_name),
            codec_id: None,
        }],
    }
}

fn value_manifest(id: &str, export: &str, target: LibTarget) -> LibManifest {
    LibManifest {
        id: Symbol::new(id),
        version: Version("0.1.0".to_owned()),
        abi: AbiVersion { major: 0, minor: 1 },
        target,
        requires: Vec::new(),
        capabilities: Vec::new(),
        exports: vec![Export::Value {
            symbol: Symbol::new(export),
        }],
    }
}

struct FixtureCodecLib {
    manifest: LibManifest,
    codec_symbol: Symbol,
}

impl FixtureCodecLib {
    fn new(id: &str, codec_name: &str, target: LibTarget) -> Self {
        Self {
            manifest: codec_manifest(id, codec_name, target),
            codec_symbol: Symbol::qualified("codec", codec_name),
        }
    }
}

impl Lib for FixtureCodecLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        linker
            .codec_value(self.codec_symbol.clone(), cx.factory().bool(true).unwrap())
            .map(|_| ())
    }
}

struct FixtureValueLib {
    manifest: LibManifest,
    export: Symbol,
}

impl FixtureValueLib {
    fn new(id: &str, export: &str, target: LibTarget) -> Self {
        Self {
            manifest: value_manifest(id, export, target),
            export: Symbol::new(export),
        }
    }
}

impl Lib for FixtureValueLib {
    fn manifest(&self) -> LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker) -> sim_kernel::Result<()> {
        linker.value(self.export.clone(), cx.factory().bool(true).unwrap())
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

fn artifact_bytes(source: KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    if let Some(bytes) = sim_run_loaders::bytes_from_source(&source)? {
        return Ok(bytes);
    }
    if let Some(path) = sim_run_loaders::path_from_source(&source)? {
        return fs::read(path).map_err(|err| KernelError::Lib(format!("read artifact: {err}")));
    }
    Err(KernelError::Lib("unsupported fixture source".to_owned()))
}

fn artifact_bytes_ref(source: &KernelLibSource) -> sim_kernel::Result<Vec<u8>> {
    if let Some(bytes) = sim_run_loaders::bytes_from_source(source)? {
        return Ok(bytes);
    }
    if let Some(path) = sim_run_loaders::path_from_source(source)? {
        return fs::read(path).map_err(|err| KernelError::Lib(format!("read artifact: {err}")));
    }
    Err(KernelError::Lib("unsupported fixture source".to_owned()))
}

fn artifact_lib(bytes: &[u8]) -> sim_kernel::Result<Box<dyn Lib>> {
    match bytes {
        b"codec-lisp" => Ok(Box::new(FixtureCodecLib::new(
            "codec-lisp",
            "lisp",
            LibTarget::DataOnly,
        ))),
        b"codec-json" => Ok(Box::new(FixtureCodecLib::new(
            "codec-json",
            "json",
            LibTarget::DataOnly,
        ))),
        b"codec-test-crate" => Ok(Box::new(FixtureCodecLib::new(
            "codec-test-crate",
            "test",
            LibTarget::DataOnly,
        ))),
        b"codec-catalog" => Ok(Box::new(FixtureCodecLib::new(
            "codec-catalog",
            "lisp",
            LibTarget::DataOnly,
        ))),
        b"ordinary-lib" => Ok(Box::new(FixtureValueLib::new(
            "ordinary-lib",
            "ordinary-value",
            LibTarget::DataOnly,
        ))),
        _ => Err(KernelError::Lib("artifact rejected".to_owned())),
    }
}

fn artifact_manifest(bytes: &[u8]) -> sim_kernel::Result<LibManifest> {
    match bytes {
        b"codec-lisp" => Ok(codec_manifest("codec-lisp", "lisp", LibTarget::DataOnly)),
        b"codec-json" => Ok(codec_manifest("codec-json", "json", LibTarget::DataOnly)),
        b"codec-test-crate" => Ok(codec_manifest(
            "codec-test-crate",
            "test",
            LibTarget::DataOnly,
        )),
        b"codec-catalog" => Ok(codec_manifest("codec-catalog", "lisp", LibTarget::DataOnly)),
        b"ordinary-lib" => Ok(value_manifest(
            "ordinary-lib",
            "ordinary-value",
            LibTarget::DataOnly,
        )),
        _ => Err(KernelError::Lib("artifact rejected".to_owned())),
    }
}

#[test]
fn default_lisp_boots_from_crates_io_cache_and_records_role() {
    let cache = temp_cache("default-lisp-codec");
    let artifact = temp_artifact("default-lisp-codec");
    fs::write(&artifact, b"codec-lisp").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-codec-lisp",
        "0.1.0",
        artifact.clone(),
    );
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_crates_io_resolver(resolver);

    let receipts = session.load_boot(&CliBoot::default()).unwrap().to_vec();

    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].role,
        LoadReceiptRole::BootCodec {
            name: "lisp".to_owned(),
            symbol: "codec/lisp".to_owned(),
        }
    );
    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::CratesIo("sim-codec-lisp@^0.1".parse().unwrap())
    );
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec-lisp"));
    assert!(
        session
            .cx()
            .registry()
            .codec_by_symbol(&Symbol::qualified("codec", "lisp"))
            .is_some()
    );
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn codec_override_uses_named_codec() {
    let cache = temp_cache("json-codec");
    let artifact = temp_artifact("json-codec");
    fs::write(&artifact, b"codec-json").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-codec-json",
        "0.1.0",
        artifact.clone(),
    );
    let boot = CliBoot {
        codec: Some("json".to_owned()),
        ..CliBoot::default()
    };
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_crates_io_resolver(resolver);

    let receipts = session.load_boot(&boot).unwrap().to_vec();

    assert_eq!(
        receipts[0].role,
        LoadReceiptRole::BootCodec {
            name: "json".to_owned(),
            symbol: "codec/json".to_owned(),
        }
    );
    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::CratesIo("sim-codec-json@^0.1".parse().unwrap())
    );
    assert!(
        session
            .cx()
            .registry()
            .codec_by_symbol(&Symbol::qualified("codec", "json"))
            .is_some()
    );
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn missing_codec_has_clear_error() {
    let cache = temp_cache("missing-codec");
    let mut session =
        LoadSession::new().with_crates_io_resolver(CratesIoResolver::new(cache.clone()));

    let err = session.load_boot(&CliBoot::default()).unwrap_err();

    assert_eq!(
        err.to_string(),
        "no codec 'lisp' available; provide one with --load \
         (crates.io network fetch is not implemented (cache-only resolver); \
         seed the cache for sim-codec-lisp@^0.1)"
    );
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn explicit_host_codec_loads_before_other_sources() {
    let cache = temp_cache("explicit-host-codec");
    let artifact = temp_artifact("explicit-host-codec");
    fs::write(&artifact, b"codec-test-crate").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-codec-test",
        "0.1.0",
        artifact.clone(),
    );
    let boot = CliBoot {
        codec: Some("test".to_owned()),
        loads: vec![
            LibSourceSpec::Host("codec/test".to_owned()),
            LibSourceSpec::Bytes(b"ordinary-lib".to_vec()),
        ],
        native_audio_provider: None,
        config: crate::ConfigLoadOptions::default(),
        list: false,
        inspect: None,
        config_report: None,
        payload: Payload::default(),
    };
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_catalog_source(
            "codec/test",
            sim_run_loaders::catalog_bytes_source(b"codec-catalog".to_vec()),
        )
        .with_crates_io_resolver(resolver)
        .with_host_factory("codec/test", || {
            Box::new(FixtureCodecLib::new(
                "codec-test-host",
                "test",
                HostRegistered,
            ))
        });

    let receipts = session.load_boot(&boot).unwrap().to_vec();

    assert_eq!(receipts.len(), 2);
    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::Host("codec/test".to_owned())
    );
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec-test-host"));
    assert_eq!(receipts[1].role, LoadReceiptRole::Library);
    assert_eq!(receipts[1].manifest.id, Symbol::new("ordinary-lib"));
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("ordinary-value"))
            .is_some()
    );
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn path_source_that_exports_requested_codec_boots_before_other_loads() {
    let path = temp_artifact("path-codec");
    fs::write(&path, b"codec-lisp").unwrap();
    let boot = CliBoot {
        loads: vec![
            LibSourceSpec::Bytes(b"ordinary-lib".to_vec()),
            LibSourceSpec::Path(path.clone()),
        ],
        ..CliBoot::default()
    };
    let mut session = LoadSession::new().with_loader(ArtifactLoader);

    let receipts = session.load_boot(&boot).unwrap().to_vec();

    assert_eq!(receipts.len(), 2);
    assert_eq!(
        receipts[0].role,
        LoadReceiptRole::BootCodec {
            name: "lisp".to_owned(),
            symbol: "codec/lisp".to_owned(),
        }
    );
    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::Path(path.clone())
    );
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec-lisp"));
    assert_eq!(receipts[1].role, LoadReceiptRole::Library);
    assert_eq!(receipts[1].manifest.id, Symbol::new("ordinary-lib"));
    let _ = fs::remove_file(path);
}

#[test]
fn catalog_source_precedes_crates_io_and_host_fallbacks() {
    let cache = temp_cache("catalog-codec");
    let artifact = temp_artifact("catalog-codec");
    fs::write(&artifact, b"codec-lisp").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-codec-lisp",
        "0.1.0",
        artifact.clone(),
    );
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_catalog_source(
            "codec/lisp",
            sim_run_loaders::catalog_bytes_source(b"codec-catalog".to_vec()),
        )
        .with_crates_io_resolver(resolver)
        .with_host_factory("codec/lisp", || {
            Box::new(FixtureCodecLib::new(
                "codec-lisp-host",
                "lisp",
                HostRegistered,
            ))
        });

    let receipts = session.load_boot(&CliBoot::default()).unwrap().to_vec();

    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::Symbol("codec/lisp".to_owned())
    );
    assert_eq!(
        receipts[0].resolved_source,
        LibSourceSpec::Bytes(b"codec-catalog".to_vec())
    );
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec-catalog"));
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn bundled_host_codec_is_last_fallback() {
    let mut session = LoadSession::new().with_host_factory("codec/lisp", || {
        Box::new(FixtureCodecLib::new(
            "codec-lisp-host",
            "lisp",
            HostRegistered,
        ))
    });

    let receipts = session.load_boot(&CliBoot::default()).unwrap().to_vec();

    assert_eq!(
        receipts[0].requested_source,
        LibSourceSpec::Host("codec/lisp".to_owned())
    );
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec-lisp-host"));
}

fn temp_artifact(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-codec-{}-{label}.artifact",
        std::process::id()
    ))
}

fn temp_cache(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-codec-cache-{}-{label}",
        std::process::id()
    ))
}
