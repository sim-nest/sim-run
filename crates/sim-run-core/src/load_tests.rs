use std::{fs, path::PathBuf};

use sim_kernel::{
    AbiVersion, Error as KernelError, Export, Lib, LibId, LibLoader, LibManifest, LibTarget,
    Linker, LoadCx, Symbol, Version,
    library::{LibSource as KernelLibSource, LibTarget::HostRegistered},
};

#[cfg(feature = "registry")]
use crate::GitRegistryResolver;
use crate::{CliBoot, CratesIoResolver, LibSourceSpec, LoadSession};

fn manifest(id: &str, export: &str, target: LibTarget) -> LibManifest {
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

struct FixtureLib {
    manifest: LibManifest,
    export: Symbol,
}

impl FixtureLib {
    fn new(id: &str, export: &str, target: LibTarget) -> Self {
        Self {
            manifest: manifest(id, export, target),
            export: Symbol::new(export),
        }
    }
}

impl Lib for FixtureLib {
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
        matches!(
            source,
            KernelLibSource::Bytes(_) | KernelLibSource::Path(_) | KernelLibSource::Url(_)
        )
    }

    fn load(
        &self,
        _cx: &mut sim_kernel::Cx,
        source: KernelLibSource,
    ) -> sim_kernel::Result<Box<dyn Lib>> {
        let bytes = match source {
            KernelLibSource::Bytes(bytes) => bytes,
            KernelLibSource::Path(path) => {
                fs::read(path).map_err(|err| KernelError::Lib(format!("read artifact: {err}")))?
            }
            KernelLibSource::Url(url) => {
                return Err(KernelError::Lib(format!("url artifact unavailable: {url}")));
            }
            KernelLibSource::Symbol(_) | KernelLibSource::Host(_) => {
                return Err(KernelError::Lib("unsupported fixture source".to_owned()));
            }
        };
        match bytes.as_slice() {
            b"bytes-lib" => Ok(Box::new(FixtureLib::new(
                "bytes-lib",
                "bytes-value",
                LibTarget::DataOnly,
            ))),
            b"path-lib" => Ok(Box::new(FixtureLib::new(
                "path-lib",
                "path-value",
                LibTarget::DataOnly,
            ))),
            b"crate-lib" => Ok(Box::new(FixtureLib::new(
                "crate-lib",
                "crate-value",
                LibTarget::DataOnly,
            ))),
            b"demo-lib" => Ok(Box::new(FixtureLib::new(
                "demo-lib",
                "demo-value",
                LibTarget::DataOnly,
            ))),
            _ => Err(KernelError::Lib("artifact rejected".to_owned())),
        }
    }
}

#[test]
fn host_source_loads_and_records_receipt() {
    let mut session = LoadSession::new().with_host_factory("test/demo", || {
        Box::new(FixtureLib::new("host-demo", "host-value", HostRegistered))
    });

    let receipt = session
        .load_source(&LibSourceSpec::Host("test/demo".to_owned()))
        .unwrap();

    assert_eq!(receipt.lib_id, LibId(1));
    assert_eq!(
        receipt.requested_source,
        LibSourceSpec::Host("test/demo".to_owned())
    );
    assert_eq!(
        receipt.resolved_source,
        LibSourceSpec::Host("test/demo".to_owned())
    );
    assert_eq!(receipt.manifest.id, Symbol::new("host-demo"));
    assert_eq!(receipt.exports.len(), 1);
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("host-value"))
            .is_some()
    );
}

#[test]
fn bytes_source_loads_through_kernel_loader_with_receipt() {
    let mut session = LoadSession::new().with_loader(ArtifactLoader);

    let receipt = session
        .load_source(&LibSourceSpec::Bytes(b"bytes-lib".to_vec()))
        .unwrap();

    assert_eq!(receipt.lib_id, LibId(1));
    assert_eq!(receipt.manifest.id, Symbol::new("bytes-lib"));
    assert_eq!(
        receipt.requested_source,
        LibSourceSpec::Bytes(b"bytes-lib".to_vec())
    );
    assert_eq!(
        receipt.resolved_source,
        LibSourceSpec::Bytes(b"bytes-lib".to_vec())
    );
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("bytes-value"))
            .is_some()
    );
}

#[test]
fn path_source_loads_existing_artifact() {
    let path = temp_artifact("path-source-loads-existing-artifact");
    fs::write(&path, b"path-lib").unwrap();
    let mut session = LoadSession::new().with_loader(ArtifactLoader);

    let receipt = session
        .load_source(&LibSourceSpec::Path(path.clone()))
        .unwrap();

    assert_eq!(receipt.manifest.id, Symbol::new("path-lib"));
    assert_eq!(receipt.requested_source, LibSourceSpec::Path(path.clone()));
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("path-value"))
            .is_some()
    );
    let _ = fs::remove_file(path);
}

#[test]
fn unknown_source_kind_is_rejected_before_loading() {
    let err = "crate:demo".parse::<LibSourceSpec>().unwrap_err();
    assert_eq!(err.to_string(), "unsupported library source kind: crate");
}

#[test]
fn load_failure_keeps_registry_empty() {
    let mut session = LoadSession::new().with_loader(ArtifactLoader);

    let err = session
        .load_source(&LibSourceSpec::Bytes(b"bad-artifact".to_vec()))
        .unwrap_err();

    assert!(err.to_string().contains("load failed for bytes:12 bytes"));
    assert!(err.to_string().contains("artifact rejected"));
    assert!(session.cx().registry().libs().is_empty());
}

#[test]
fn missing_path_source_is_reported_clearly() {
    let path = temp_artifact("missing-path-source");
    let mut session = LoadSession::new().with_loader(ArtifactLoader);

    let err = session
        .load_source(&LibSourceSpec::Path(path.clone()))
        .unwrap_err();

    assert!(err.to_string().contains("path source not found"));
    assert!(err.to_string().contains(&path.display().to_string()));
}

#[test]
fn crates_io_source_records_requested_and_resolved_receipt() {
    let cache = temp_cache("crates-io-receipt");
    let artifact = temp_artifact("crates-io-receipt-source");
    fs::write(&artifact, b"crate-lib").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-lib-crate",
        "0.1.0",
        artifact.clone(),
    );
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_crates_io_resolver(resolver);
    let source = LibSourceSpec::CratesIo("sim-lib-crate@0.1.0".parse().unwrap());

    let receipt = session.load_source(&source).unwrap();

    assert_eq!(receipt.requested_source, source);
    assert!(matches!(receipt.resolved_source, LibSourceSpec::Path(_)));
    assert_eq!(receipt.manifest.id, Symbol::new("crate-lib"));
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("crate-value"))
            .is_some()
    );
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[test]
fn symbol_fallback_resolves_through_crates_io() {
    let cache = temp_cache("symbol-fallback");
    let artifact = temp_artifact("symbol-fallback-source");
    fs::write(&artifact, b"demo-lib").unwrap();
    let resolver = CratesIoResolver::new(cache.clone()).with_registry_artifact(
        "sim-lib-demo",
        "0.1.0",
        artifact.clone(),
    );
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_crates_io_resolver(resolver);

    let receipt = session
        .load_source(&LibSourceSpec::Symbol("demo".to_owned()))
        .unwrap();

    assert_eq!(
        receipt.requested_source,
        LibSourceSpec::CratesIo("sim-lib-demo@^0.1".parse().unwrap())
    );
    assert_eq!(receipt.manifest.id, Symbol::new("demo-lib"));
    let _ = fs::remove_dir_all(cache);
    let _ = fs::remove_file(artifact);
}

#[cfg(feature = "registry")]
#[test]
fn symbol_fallback_resolves_through_git_registry() {
    let server = FixtureServer::start([
        ("/packages/sim-lib-demo/index.txt", b"0.1.0\n".to_vec()),
        (
            "/packages/sim-lib-demo/0.1.0/artifact.simlib",
            b"demo-lib".to_vec(),
        ),
    ]);
    let cache = temp_cache("symbol-git-registry");
    let resolver = CratesIoResolver::new(cache.clone()).with_git_registry_resolver(
        GitRegistryResolver::new(server.endpoint(), cache.clone()).unwrap(),
    );
    let mut session = LoadSession::new()
        .with_loader(ArtifactLoader)
        .with_crates_io_resolver(resolver);

    let receipt = session
        .load_source(&LibSourceSpec::Symbol("demo".to_owned()))
        .unwrap();

    assert_eq!(
        receipt.requested_source,
        LibSourceSpec::CratesIo("sim-lib-demo@^0.1".parse().unwrap())
    );
    assert_eq!(receipt.manifest.id, Symbol::new("demo-lib"));
    assert!(matches!(receipt.resolved_source, LibSourceSpec::Path(_)));
    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("demo-value"))
            .is_some()
    );
    server.join();
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn unload_receipt_cleans_up_loaded_exports() {
    let mut session = LoadSession::new().with_host_factory("test/demo", || {
        Box::new(FixtureLib::new("host-demo", "host-value", HostRegistered))
    });
    let receipt = session
        .load_source(&LibSourceSpec::Host("test/demo".to_owned()))
        .unwrap();

    assert_eq!(
        session.unload_receipt(&receipt).unwrap(),
        vec![receipt.lib_id]
    );

    assert!(
        session
            .cx()
            .registry()
            .value_by_symbol(&Symbol::new("host-value"))
            .is_none()
    );
}

#[test]
fn missing_native_audio_provider_does_not_block_boot() {
    let mut session = LoadSession::new().with_host_factory("codec/lisp", || {
        Box::new(FixtureLib::new("codec/lisp", "codec-value", HostRegistered))
    });
    let boot = CliBoot {
        native_audio_provider: Some(Box::new(LibSourceSpec::Path(temp_artifact(
            "missing-native-audio-provider",
        )))),
        ..CliBoot::default()
    };

    let receipts = session.load_boot(&boot).unwrap().to_vec();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].manifest.id, Symbol::new("codec/lisp"));
    assert!(!session.native_audio_provider_active());
    assert!(
        session
            .cx()
            .require(&sim_lib_stream_host::native_audio_provider_capability())
            .is_err()
    );
    assert!(
        session
            .config_state()
            .diagnostics()
            .iter()
            .any(|diagnostic| {
                diagnostic.starts_with("native audio provider skipped: path source not found")
            })
    );
}

#[test]
fn loaded_native_audio_provider_is_active_and_granted() {
    let mut session = LoadSession::new()
        .with_host_factory("codec/lisp", || {
            Box::new(FixtureLib::new("codec/lisp", "codec-value", HostRegistered))
        })
        .with_host_factory("audio/provider/jack", || {
            Box::new(FixtureLib::new(
                "audio/provider/jack",
                "native-provider-value",
                HostRegistered,
            ))
        });
    let boot = CliBoot {
        native_audio_provider: Some(Box::new(LibSourceSpec::Host(
            "audio/provider/jack".to_owned(),
        ))),
        ..CliBoot::default()
    };

    session.load_boot(&boot).unwrap();

    assert!(session.native_audio_provider_active());
    assert!(
        session
            .cx()
            .require(&sim_lib_stream_host::native_audio_provider_capability())
            .is_ok()
    );
}

#[test]
fn default_boot_does_not_grant_native_audio_provider_capability() {
    let mut session = LoadSession::new().with_host_factory("codec/lisp", || {
        Box::new(FixtureLib::new("codec/lisp", "codec-value", HostRegistered))
    });

    session.load_boot(&CliBoot::default()).unwrap();

    assert!(!session.native_audio_provider_active());
    assert!(
        session
            .cx()
            .require(&sim_lib_stream_host::native_audio_provider_capability())
            .is_err()
    );
}

fn temp_artifact(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sim-run-core-{}-{label}.artifact",
        std::process::id()
    ))
}

fn temp_cache(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-run-core-cache-{}-{label}-{nanos}",
        std::process::id(),
    ))
}

#[cfg(feature = "registry")]
struct FixtureServer {
    endpoint: String,
    handle: std::thread::JoinHandle<()>,
}

#[cfg(feature = "registry")]
impl FixtureServer {
    fn start<const N: usize>(routes: [(&'static str, Vec<u8>); N]) -> Self {
        use std::{
            collections::BTreeMap,
            io::{Read, Write},
            net::TcpListener,
            thread,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let routes = routes
            .into_iter()
            .map(|(path, body)| (path.to_owned(), body))
            .collect::<BTreeMap<_, _>>();
        let handle = thread::spawn(move || {
            for _ in 0..N {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 1024];
                let size = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap();
                let Some(body) = routes.get(path) else {
                    stream
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                    continue;
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self { endpoint, handle }
    }

    fn endpoint(&self) -> String {
        self.endpoint.clone()
    }

    fn join(self) {
        self.handle.join().unwrap();
    }
}
