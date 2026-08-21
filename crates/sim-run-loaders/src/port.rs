//! Run-owned domain boundary for turning exact artifacts into libraries.

use std::sync::Arc;

use sim_kernel::{Cx, Lib, LibLoader, LibManifest, LibSource, Result, Symbol};

/// Source kind used by data-backed AOT registries.
pub const STATIC_SOURCE_KIND: &str = "loader/static-artifact";

fn static_source_kind() -> Symbol {
    Symbol::qualified("loader", "static-artifact")
}

/// Creates an exact AOT artifact source.
#[must_use]
pub fn static_source(artifact: Symbol) -> LibSource {
    LibSource::open(static_source_kind(), sim_kernel::Datum::Symbol(artifact))
}

/// Returns whether a source names an exact AOT artifact.
#[must_use]
pub fn is_static_source(source: &LibSource) -> bool {
    matches!(source, LibSource::Open { kind, payload: sim_kernel::Datum::Symbol(_) }
        if kind == &static_source_kind())
}

/// Decodes an exact AOT artifact identity.
pub fn static_artifact(source: &LibSource) -> Result<Option<Symbol>> {
    match source {
        LibSource::Open {
            kind,
            payload: sim_kernel::Datum::Symbol(artifact),
        } if kind == &static_source_kind() => Ok(Some(artifact.clone())),
        LibSource::Open { kind, .. } if kind == &static_source_kind() => {
            Err(sim_kernel::Error::HostError(
                "static artifact source requires a symbol payload".to_owned(),
            ))
        }
        _ => Ok(None),
    }
}

/// Open loader-kind identity advertised by an installed platform capsule.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LoaderKind(Symbol);

impl LoaderKind {
    /// Creates a loader kind from its stable symbol.
    #[must_use]
    pub fn new(symbol: Symbol) -> Self {
        Self(symbol)
    }

    /// Returns the stable loader-kind symbol.
    #[must_use]
    pub fn symbol(&self) -> &Symbol {
        &self.0
    }
}

/// Exact, data-only request delivered to a platform loader service.
pub struct LoadRequest {
    /// Loader kind selected from the capsule Card.
    pub kind: LoaderKind,
    /// Exact source named by the bundle descriptor.
    pub source: LibSource,
}

/// Successful realization returned by an installed loader service.
pub struct LoadOutcome {
    /// Manifest obtained from the realized artifact.
    pub manifest: LibManifest,
    /// Library whose ordinary `Lib::load` and `Lib::unload` behavior is used.
    pub library: Box<dyn Lib>,
}

/// Object-safe service implemented by model, native, wasm, source, and AOT adapters.
///
/// The port deliberately reuses [`LibSource`], [`LibManifest`], and the kernel
/// [`Lib`] lifecycle. It introduces neither a second manifest nor a second ABI.
pub trait LoaderPort: Send + Sync {
    /// Loader kinds this service advertises on its capsule Card.
    fn loader_kinds(&self) -> Vec<LoaderKind>;

    /// Realizes one exact request or fails closed.
    fn realize(&self, cx: &mut Cx, request: LoadRequest) -> Result<LoadOutcome>;

    /// Inspects a request without retaining a live library where supported.
    fn inspect(&self, cx: &mut Cx, request: &LoadRequest) -> Result<Option<LibManifest>>;
}

/// Kernel-loader bridge for one exact loader kind on a full loader port.
pub struct PortLoader {
    port: Arc<dyn LoaderPort>,
    kind: LoaderKind,
    accepts: fn(&LibSource) -> bool,
}

impl PortLoader {
    /// Binds one advertised loader kind to an exact source-kind predicate.
    #[must_use]
    pub fn new(
        port: Arc<dyn LoaderPort>,
        kind: LoaderKind,
        accepts: fn(&LibSource) -> bool,
    ) -> Self {
        Self {
            port,
            kind,
            accepts,
        }
    }
}

impl LibLoader for PortLoader {
    fn can_load(&self, source: &LibSource) -> bool {
        self.port.loader_kinds().contains(&self.kind) && (self.accepts)(source)
    }

    fn load(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        let outcome = self.port.realize(
            cx,
            LoadRequest {
                kind: self.kind.clone(),
                source,
            },
        )?;
        if outcome.library.manifest() != outcome.manifest {
            return Err(sim_kernel::Error::HostError(
                "loader outcome manifest does not match realized library".to_owned(),
            ));
        }
        Ok(outcome.library)
    }

    fn inspect_manifest(&self, cx: &mut Cx, source: &LibSource) -> Result<Option<LibManifest>> {
        self.port.inspect(
            cx,
            &LoadRequest {
                kind: self.kind.clone(),
                source: clone_data_source(source)?,
            },
        )
    }
}

fn clone_data_source(source: &LibSource) -> Result<LibSource> {
    match source {
        LibSource::Symbol(symbol) => Ok(LibSource::Symbol(symbol.clone())),
        LibSource::Open { kind, payload } => Ok(LibSource::Open {
            kind: kind.clone(),
            payload: payload.clone(),
        }),
        LibSource::Host(_) => Err(sim_kernel::Error::HostError(
            "host library sources cannot cross LoaderPort".to_owned(),
        )),
    }
}

/// Data-backed AOT registry using the same manifest and call behavior as every other loader.
#[derive(Default)]
pub struct StaticRegistry {
    entries: std::sync::RwLock<
        std::collections::BTreeMap<Symbol, Arc<dyn Fn() -> Box<dyn Lib> + Send + Sync>>,
    >,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::{AbiVersion, LibTarget, Linker, LoadCx, Version};

    struct StaticLib(LibManifest);

    impl Lib for StaticLib {
        fn manifest(&self) -> LibManifest {
            self.0.clone()
        }

        fn load(&self, _: &mut LoadCx, _: &mut Linker<'_>) -> Result<()> {
            Ok(())
        }
    }

    fn manifest() -> LibManifest {
        LibManifest {
            id: Symbol::qualified("test", "static"),
            version: Version("1.0.0".to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: vec![],
        }
    }

    #[test]
    fn static_registry_is_exact_and_uses_the_library_manifest() {
        let registry = StaticRegistry::default();
        let artifact = Symbol::qualified("artifact", "static-test");
        registry.register(artifact.clone(), || Box::new(StaticLib(manifest())));
        let source = static_source(artifact.clone());
        assert!(is_static_source(&source));
        assert_eq!(static_artifact(&source).unwrap(), Some(artifact.clone()));
        let outcome = registry.realize(&artifact).unwrap();
        assert_eq!(outcome.manifest, manifest());
        assert_eq!(outcome.library.manifest(), manifest());
        assert!(
            registry
                .realize(&Symbol::qualified("artifact", "missing"))
                .is_err()
        );
    }

    #[test]
    fn malformed_static_source_fails_closed() {
        let source = LibSource::open(static_source_kind(), sim_kernel::Datum::Bytes(vec![]));
        assert!(static_artifact(&source).is_err());
    }
}

impl StaticRegistry {
    /// Registers an AOT factory under an exact artifact symbol.
    pub fn register(
        &self,
        artifact: Symbol,
        factory: impl Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    ) {
        self.entries
            .write()
            .expect("static loader registry poisoned")
            .insert(artifact, Arc::new(factory));
    }

    /// Realizes an exact registered artifact.
    pub fn realize(&self, artifact: &Symbol) -> Result<LoadOutcome> {
        let factory = self
            .entries
            .read()
            .expect("static loader registry poisoned")
            .get(artifact)
            .cloned()
            .ok_or_else(|| {
                sim_kernel::Error::HostError(format!(
                    "no static library artifact registered as {artifact}"
                ))
            })?;
        let library = factory();
        let manifest = library.manifest();
        Ok(LoadOutcome { manifest, library })
    }
}
