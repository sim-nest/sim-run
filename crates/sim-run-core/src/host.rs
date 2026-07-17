use std::collections::BTreeMap;

use sim_kernel::{
    Cx, Error as KernelError, Lib, LibBootDependency, LibLoader, LibManifest,
    LibSource as KernelLibSource, LoadedLib,
};

use crate::{CliError, LibSourceSpec, LoadReceipt, LoadReceiptRole, RuntimeConfigState};

type PlainHostFactory = Box<dyn Fn() -> Box<dyn Lib> + Send + Sync>;
type ConfigHostFactory = Box<dyn Fn(&RuntimeConfigState) -> Box<dyn Lib> + Send + Sync>;

enum HostFactory {
    Plain(PlainHostFactory),
    Config(ConfigHostFactory),
}

#[derive(Default)]
pub(crate) struct HostLibRegistry {
    factories: BTreeMap<String, HostFactory>,
}

impl HostLibRegistry {
    pub(crate) fn add(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    ) {
        self.factories
            .insert(name.into(), HostFactory::Plain(Box::new(factory)));
    }

    pub(crate) fn add_with_config(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&RuntimeConfigState) -> Box<dyn Lib> + Send + Sync + 'static,
    ) {
        self.factories
            .insert(name.into(), HostFactory::Config(Box::new(factory)));
    }

    pub(crate) fn instantiate(
        &self,
        name: &str,
        config: &RuntimeConfigState,
    ) -> Result<Box<dyn Lib>, CliError> {
        self.factories
            .get(name)
            .map(|factory| match factory {
                HostFactory::Plain(factory) => factory(),
                HostFactory::Config(factory) => factory(config),
            })
            .ok_or_else(|| CliError::new(format!("unknown host library: {name}")))
    }

    pub(crate) fn inspect_manifest(
        &self,
        name: &str,
        config: &RuntimeConfigState,
    ) -> Result<LibManifest, CliError> {
        Ok(self.instantiate(name, config)?.manifest())
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }
}

pub(crate) struct HostSourceLoader;

impl LibLoader for HostSourceLoader {
    fn can_load(&self, source: &KernelLibSource) -> bool {
        matches!(source, KernelLibSource::Host(_))
    }

    fn load(&self, _cx: &mut Cx, source: KernelLibSource) -> sim_kernel::Result<Box<dyn Lib>> {
        match source {
            KernelLibSource::Host(lib) => Ok(lib),
            _ => Err(KernelError::Lib(
                "host loader received a non-host source".to_owned(),
            )),
        }
    }
}

pub(crate) fn host_receipt(
    source: LibSourceSpec,
    role: LoadReceiptRole,
    loaded: LoadedLib,
    loaded_libs: &[LoadedLib],
) -> LoadReceipt {
    let dependencies = loaded
        .manifest
        .requires
        .iter()
        .filter_map(|dependency| {
            let loaded = loaded_libs
                .iter()
                .find(|candidate| candidate.manifest.id == dependency.id)?;
            Some(LibBootDependency {
                lib_id: loaded.id,
                symbol: loaded.manifest.id.clone(),
            })
        })
        .collect();
    LoadReceipt {
        lib_id: loaded.id,
        role,
        requested_source: source.clone(),
        resolved_source: source,
        manifest: loaded.manifest,
        dependencies,
        exports: loaded.exports,
    }
}
