use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sim_kernel::{
    CapabilityName, CatalogSource, Cx, DefaultFactory, Error as KernelError, GrantSeat, Lib, LibId,
    LibLoader, LibManifest, LibSource as KernelLibSource, LibSourceSpec as KernelLibSourceSpec,
    LoaderRegistry, NoopEvalPolicy, Symbol,
};
use sim_lib_stream_host::native_audio_provider_capability;

use crate::{
    CliBoot, CliError, ConfigReportKind, CratesIoResolver, CratesIoSpec, LibSourceSpec,
    LoadReceipt, LoadReceiptRole,
    codec_boot::{boot_codec_name, codec_lib_symbol, explicit_codec_source_index},
    config::{RuntimeConfigState, load_config_sources},
    crates_io::fallback_spec_for_symbol,
    host::{HostLibRegistry, HostSourceLoader, host_receipt},
    source::symbol_from_text,
};

/// Kernel-backed loader session used by the command entry API.
pub struct LoadSession {
    cx: Cx,
    /// Host-only grant seat minted with `cx`; the only capability-grant authority
    /// in this session. It is never handed to a loaded callable, so a loaded lib
    /// cannot mint its own capabilities.
    seat: GrantSeat,
    loaders: LoaderRegistry,
    hosts: HostLibRegistry,
    crates_io: CratesIoResolver,
    catalog_sources: BTreeMap<Symbol, LibSourceSpec>,
    default_verb_sources: BTreeMap<String, Vec<LibSourceSpec>>,
    default_verb_config_libs: BTreeMap<String, Vec<Symbol>>,
    receipts: Vec<LoadReceipt>,
    config: RuntimeConfigState,
}

trait GrantOutcome {
    fn expect_granted(self);
}

impl GrantOutcome for () {
    fn expect_granted(self) {}
}

impl GrantOutcome for Result<(), KernelError> {
    fn expect_granted(self) {
        self.expect("load session grant seat grants into its own Cx");
    }
}

macro_rules! expect_granted {
    ($grant:expr) => {{
        #[allow(clippy::let_unit_value)]
        let grant_result = $grant;
        #[allow(clippy::unit_arg)]
        grant_result.expect_granted();
    }};
}

impl LoadSession {
    /// Builds a loader session with an empty static host catalog.
    pub fn new() -> Self {
        let mut loaders = LoaderRegistry::new();
        loaders.add_loader(HostSourceLoader);
        let (cx, seat) = Cx::new_seated(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        Self {
            cx,
            seat,
            loaders,
            hosts: HostLibRegistry::default(),
            crates_io: CratesIoResolver::default(),
            catalog_sources: BTreeMap::new(),
            default_verb_sources: BTreeMap::new(),
            default_verb_config_libs: BTreeMap::new(),
            receipts: Vec::new(),
            config: RuntimeConfigState::default(),
        }
    }

    /// Adds a kernel loader to the session.
    pub fn add_loader(&mut self, loader: impl LibLoader + 'static) {
        self.loaders.add_loader(loader);
    }

    /// Registers a catalog source for a library symbol.
    pub fn add_catalog_source(&mut self, symbol: impl AsRef<str>, source: CatalogSource) {
        let symbol = symbol_from_text(symbol.as_ref());
        self.catalog_sources
            .insert(symbol.clone(), catalog_source_spec(source.clone()));
        self.loaders.add_source(symbol, source);
    }

    /// Registers a catalog source, builder-style.
    pub fn with_catalog_source(mut self, symbol: impl AsRef<str>, source: CatalogSource) -> Self {
        self.add_catalog_source(symbol, source);
        self
    }

    /// Adds a kernel loader, builder-style.
    pub fn with_loader(mut self, loader: impl LibLoader + 'static) -> Self {
        self.add_loader(loader);
        self
    }

    /// Adds a host library factory to the static host catalog.
    pub fn add_host_factory(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    ) {
        self.hosts.add(name, factory);
    }

    /// Adds a host library factory that can inspect the discovered effective
    /// runtime config before it builds the library.
    pub fn add_host_factory_with_config(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&RuntimeConfigState) -> Box<dyn Lib> + Send + Sync + 'static,
    ) {
        self.hosts.add_with_config(name, factory);
    }

    /// Adds a host library factory, builder-style.
    pub fn with_host_factory(
        mut self,
        name: impl Into<String>,
        factory: impl Fn() -> Box<dyn Lib> + Send + Sync + 'static,
    ) -> Self {
        self.add_host_factory(name, factory);
        self
    }

    /// Adds a config-aware host library factory, builder-style.
    pub fn with_host_factory_with_config(
        mut self,
        name: impl Into<String>,
        factory: impl Fn(&RuntimeConfigState) -> Box<dyn Lib> + Send + Sync + 'static,
    ) -> Self {
        self.add_host_factory_with_config(name, factory);
        self
    }

    /// Replaces the crates.io resolver used for `crates.io:` sources.
    pub fn with_crates_io_resolver(mut self, resolver: CratesIoResolver) -> Self {
        self.crates_io = resolver;
        self
    }

    /// Applies direct access to the session context, builder-style.
    ///
    /// Hosts use this to install context-level runtime support while keeping
    /// concrete command behavior in loaded libraries.
    pub fn with_context(mut self, configure: impl FnOnce(&mut Cx)) -> Self {
        configure(&mut self.cx);
        self
    }

    /// Grants a capability to the session's kernel context, builder-style.
    ///
    /// Loaders that require a capability (for example the native dynamic-library
    /// loader requires `native_dynamic_load_capability()`) only succeed when the
    /// host has granted it. This lets a composed `sim` build authorize the
    /// loaders it registers.
    pub fn with_capability(mut self, capability: CapabilityName) -> Self {
        expect_granted!(self.seat.grant(&mut self.cx, capability));
        self
    }

    /// Registers sources used when `verb` is selected without explicit loads.
    pub fn add_default_verb_sources(
        &mut self,
        verb: impl Into<String>,
        sources: Vec<LibSourceSpec>,
    ) {
        self.default_verb_sources.insert(verb.into(), sources);
    }

    /// Registers config libraries read when `verb` is selected without explicit
    /// loads.
    pub fn add_default_verb_config_libs(&mut self, verb: impl Into<String>, libs: Vec<Symbol>) {
        self.default_verb_config_libs.insert(verb.into(), libs);
    }

    /// Registers sources used when `verb` is selected without explicit loads,
    /// builder-style.
    pub fn with_default_verb_sources(
        mut self,
        verb: impl Into<String>,
        sources: Vec<LibSourceSpec>,
    ) -> Self {
        self.add_default_verb_sources(verb, sources);
        self
    }

    /// Registers config libraries read when `verb` is selected without explicit
    /// loads, builder-style.
    pub fn with_default_verb_config_libs(
        mut self,
        verb: impl Into<String>,
        libs: Vec<Symbol>,
    ) -> Self {
        self.add_default_verb_config_libs(verb, libs);
        self
    }

    /// Returns the active kernel context.
    pub fn cx(&self) -> &Cx {
        &self.cx
    }

    pub(crate) fn cx_mut(&mut self) -> &mut Cx {
        &mut self.cx
    }

    pub(crate) fn crates_io(&self) -> &CratesIoResolver {
        &self.crates_io
    }

    pub(crate) fn hosts(&self) -> &HostLibRegistry {
        &self.hosts
    }

    pub(crate) fn catalog_sources(&self) -> &BTreeMap<Symbol, LibSourceSpec> {
        &self.catalog_sources
    }

    pub(crate) fn resolve_data_source(&self, source: KernelLibSourceSpec) -> KernelLibSourceSpec {
        self.loaders.resolve_source_spec(&source)
    }

    pub(crate) fn inspect_data_source_manifest(
        &mut self,
        source: KernelLibSourceSpec,
    ) -> Result<LibManifest, CliError> {
        self.loaders
            .inspect_manifest(&mut self.cx, source.into())
            .map_err(|err| CliError::new(format!("inspect source: {err}")))
    }

    /// Returns load receipts in boot order.
    pub fn receipts(&self) -> &[LoadReceipt] {
        &self.receipts
    }

    /// Returns the runtime config state discovered during boot.
    pub fn config_state(&self) -> &RuntimeConfigState {
        &self.config
    }

    /// Returns the runtime config state mutably for host integration.
    pub fn config_state_mut(&mut self) -> &mut RuntimeConfigState {
        &mut self.config
    }

    /// Loads every requested library in the parsed boot controls.
    pub fn load_boot(&mut self, boot: &CliBoot) -> Result<&[LoadReceipt], CliError> {
        let boot = self.boot_with_default_verb_sources(boot);
        let config_libs = config_libs_for_boot(&boot, &self.default_verb_config_libs);
        let codec_name = boot_codec_name(&boot);
        let codec_symbol = codec_lib_symbol(codec_name);
        let codec_index = self.boot_codec_source_index(&boot, &codec_symbol);
        self.config = load_config_sources(&mut self.cx, &pre_site_config(&boot), &config_libs);
        let preloaded_config_sources =
            self.load_config_site_sources(&boot, codec_name, &codec_symbol, codec_index)?;
        self.config = load_config_sources(&mut self.cx, &boot.config, &config_libs);
        self.load_native_audio_provider(&boot);
        match codec_index {
            Some(index) if preloaded_config_sources.contains(&index) => {}
            Some(index) => {
                self.load_boot_codec_source(codec_name, &codec_symbol, &boot.loads[index])?;
            }
            None => {
                self.load_boot_codec_source(
                    codec_name,
                    &codec_symbol,
                    &LibSourceSpec::Symbol(codec_symbol.clone()),
                )?;
            }
        }
        for (index, source) in boot.loads.iter().enumerate() {
            if Some(index) != codec_index && !preloaded_config_sources.contains(&index) {
                self.load_source(source)?;
            }
        }
        Ok(&self.receipts)
    }

    fn load_config_site_sources(
        &mut self,
        boot: &CliBoot,
        codec_name: &str,
        codec_symbol: &str,
        codec_index: Option<usize>,
    ) -> Result<BTreeSet<usize>, CliError> {
        let mut preloaded = BTreeSet::new();
        if boot.config.site_sources.is_empty() {
            return Ok(preloaded);
        }
        for (index, source) in boot.loads.iter().enumerate() {
            let Ok(manifest) = self.inspect_source_manifest(source) else {
                continue;
            };
            if !manifest_exports_requested_site(&manifest, &boot.config.site_sources) {
                continue;
            }
            if Some(index) == codec_index {
                self.load_boot_codec_source(codec_name, codec_symbol, source)?;
            } else {
                self.load_source(source)?;
            }
            preloaded.insert(index);
        }
        Ok(preloaded)
    }

    fn load_native_audio_provider(&mut self, boot: &CliBoot) {
        let Some(source) = boot.native_audio_provider.as_deref() else {
            return;
        };
        expect_granted!(
            self.seat
                .grant(&mut self.cx, native_audio_provider_capability())
        );
        if self
            .load_source_with_role(source, LoadReceiptRole::Library)
            .is_err()
        {
            // Placement resolution keeps the modeled site live when a native
            // provider is absent or rejected.
        }
    }

    fn boot_with_default_verb_sources(&self, boot: &CliBoot) -> CliBoot {
        if !boot.loads.is_empty() {
            return boot.clone();
        }
        let Some(verb) = boot
            .payload
            .args
            .first()
            .map(|arg| arg.to_string_lossy().into_owned())
        else {
            return boot.clone();
        };
        let Some(sources) = self.default_verb_sources.get(&verb) else {
            return boot.clone();
        };
        let mut boot = boot.clone();
        boot.loads.clone_from(sources);
        boot
    }

    fn boot_codec_source_index(&mut self, boot: &CliBoot, codec_symbol: &str) -> Option<usize> {
        if let Some(index) = explicit_codec_source_index(boot, codec_symbol) {
            return Some(index);
        }

        let codec_symbol = symbol_from_text(codec_symbol);
        for (index, source) in boot.loads.iter().enumerate() {
            // Best-effort manifest inspection lets path/bytes/crates.io sources
            // supply the boot codec without blocking on unrelated load errors.
            if self
                .inspect_source_manifest(source)
                .ok()
                .is_some_and(|manifest| manifest_exports_codec(&manifest, &codec_symbol))
            {
                return Some(index);
            }
        }
        None
    }

    /// Loads one source through the kernel loader and records a receipt.
    pub fn load_source(&mut self, source: &LibSourceSpec) -> Result<LoadReceipt, CliError> {
        self.load_source_with_role(source, LoadReceiptRole::Library)
    }

    fn inspect_source_manifest(&mut self, source: &LibSourceSpec) -> Result<LibManifest, CliError> {
        match source {
            LibSourceSpec::Host(name) => self.hosts.inspect_manifest(name, &self.config),
            LibSourceSpec::CratesIo(spec) => {
                let resolved = self.crates_io.resolve(spec)?;
                let data_source = KernelLibSourceSpec::Path(resolved.artifact);
                ensure_loadable_path(&data_source, source)?;
                self.inspect_data_source_manifest(data_source)
            }
            _ => {
                let data_source = source
                    .to_kernel_data_source()
                    .expect("non-host sources have data forms");
                ensure_loadable_path(&data_source, source)?;
                self.inspect_data_source_manifest(data_source)
            }
        }
    }

    fn load_boot_codec_source(
        &mut self,
        codec_name: &str,
        codec_symbol: &str,
        source: &LibSourceSpec,
    ) -> Result<LoadReceipt, CliError> {
        let role = LoadReceiptRole::boot_codec(codec_name, codec_symbol);
        match self.load_source_with_role(source, role.clone()) {
            Ok(receipt) => Ok(receipt),
            Err(_) if self.hosts.contains(codec_symbol) => {
                self.load_source_with_role(&LibSourceSpec::Host(codec_symbol.to_owned()), role)
            }
            Err(err) => Err(no_codec_error(codec_name, err)),
        }
    }

    fn load_source_with_role(
        &mut self,
        source: &LibSourceSpec,
        role: LoadReceiptRole,
    ) -> Result<LoadReceipt, CliError> {
        if let LibSourceSpec::Host(name) = source {
            return self.load_host_source(source, name, role);
        }
        if let LibSourceSpec::CratesIo(spec) = source {
            return self.load_crates_io_source(source, spec, role);
        }

        let data_source = source
            .to_kernel_data_source()
            .expect("non-host sources have data forms");
        ensure_loadable_path(&data_source, source)?;
        let fallback = match source {
            LibSourceSpec::Symbol(symbol) => fallback_spec_for_symbol(symbol),
            _ => None,
        };
        match self.load_data_source(source, data_source, role.clone()) {
            Ok(receipt) => Ok(receipt),
            Err(err) => match fallback {
                Some(spec) => {
                    self.load_crates_io_source(&LibSourceSpec::CratesIo(spec.clone()), &spec, role)
                }
                None => Err(err),
            },
        }
    }

    fn load_data_source(
        &mut self,
        source: &LibSourceSpec,
        data_source: KernelLibSourceSpec,
        role: LoadReceiptRole,
    ) -> Result<LoadReceipt, CliError> {
        let receipt = self
            .loaders
            .load_and_register_with_receipt(&mut self.cx, data_source)
            .map_err(|err| load_error(source, err))?;
        let receipt = LoadReceipt {
            lib_id: receipt.lib_id,
            role,
            requested_source: LibSourceSpec::from_kernel_data_source(receipt.requested_source),
            resolved_source: LibSourceSpec::from_kernel_data_source(receipt.resolved_source),
            manifest: receipt.manifest,
            dependencies: receipt.dependencies,
            exports: receipt.exports,
        };
        self.receipts.push(receipt.clone());
        Ok(receipt)
    }

    fn load_crates_io_source(
        &mut self,
        source: &LibSourceSpec,
        spec: &CratesIoSpec,
        role: LoadReceiptRole,
    ) -> Result<LoadReceipt, CliError> {
        let resolved = self.crates_io.resolve(spec)?;
        let data_source = KernelLibSourceSpec::Path(resolved.artifact);
        ensure_loadable_path(&data_source, source)?;
        let receipt = self
            .loaders
            .load_and_register_with_receipt(&mut self.cx, data_source)
            .map_err(|err| load_error(source, err))?;
        let receipt = LoadReceipt {
            lib_id: receipt.lib_id,
            role,
            requested_source: source.clone(),
            resolved_source: LibSourceSpec::from_kernel_data_source(receipt.resolved_source),
            manifest: receipt.manifest,
            dependencies: receipt.dependencies,
            exports: receipt.exports,
        };
        self.receipts.push(receipt.clone());
        Ok(receipt)
    }

    /// Unloads a receipt's library through the kernel lifecycle path.
    pub fn unload_receipt(&mut self, receipt: &LoadReceipt) -> Result<Vec<LibId>, CliError> {
        self.cx.unload_lib(receipt.lib_id).map_err(|err| {
            CliError::new(format!("unload failed for {}: {err}", receipt.manifest.id))
        })
    }

    fn load_host_source(
        &mut self,
        source: &LibSourceSpec,
        name: &str,
        role: LoadReceiptRole,
    ) -> Result<LoadReceipt, CliError> {
        let lib = self.hosts.instantiate(name, &self.config)?;
        let lib_id = self
            .loaders
            .load_and_register(&mut self.cx, KernelLibSource::Host(lib))
            .map_err(|err| load_error(source, err))?;
        let loaded = self
            .cx
            .registry()
            .libs()
            .iter()
            .find(|loaded| loaded.id == lib_id)
            .cloned()
            .ok_or_else(|| CliError::new(format!("loaded lib id {lib_id:?} is not registered")))?;
        let receipt = host_receipt(source.clone(), role, loaded, self.cx.registry().libs());
        self.receipts.push(receipt.clone());
        Ok(receipt)
    }
}

fn catalog_source_spec(source: CatalogSource) -> LibSourceSpec {
    match source {
        CatalogSource::Path(path) => LibSourceSpec::Path(path),
        CatalogSource::Url(url) => LibSourceSpec::Url(url),
        CatalogSource::Bytes(bytes) => LibSourceSpec::Bytes(bytes),
    }
}

fn config_libs_for_boot(
    boot: &CliBoot,
    default_verb_config_libs: &BTreeMap<String, Vec<Symbol>>,
) -> Vec<Symbol> {
    let codec_name = boot_codec_name(boot);
    let mut libs = Vec::new();
    push_unique_symbol(&mut libs, symbol_from_text(&codec_lib_symbol(codec_name)));
    for source in &boot.loads {
        if let Some(symbol) = config_lib_for_source(source) {
            push_unique_symbol(&mut libs, symbol);
        }
    }
    if let Some(source) = boot.native_audio_provider.as_deref()
        && let Some(symbol) = config_lib_for_source(source)
    {
        push_unique_symbol(&mut libs, symbol);
    }
    if let Some(verb) = boot
        .payload
        .args
        .first()
        .map(|arg| arg.to_string_lossy().into_owned())
        && let Some(symbols) = default_verb_config_libs.get(&verb)
    {
        for symbol in symbols {
            push_unique_symbol(&mut libs, symbol.clone());
        }
    }
    if let Some(request) = boot.config_report.as_ref() {
        match &request.kind {
            ConfigReportKind::Effective { lib } => push_unique_symbol(&mut libs, lib.clone()),
            ConfigReportKind::Status | ConfigReportKind::Sources => {
                for lib in representative_config_report_libs() {
                    push_unique_symbol(&mut libs, lib);
                }
            }
        }
    }
    libs
}

fn representative_config_report_libs() -> [Symbol; 3] {
    [
        Symbol::qualified("sim", "cookbook"),
        Symbol::qualified("stream", "host"),
        Symbol::qualified("model", "defaults"),
    ]
}

fn config_lib_for_source(source: &LibSourceSpec) -> Option<Symbol> {
    match source {
        LibSourceSpec::Symbol(symbol) | LibSourceSpec::Host(symbol) => {
            Some(symbol_from_text(symbol))
        }
        LibSourceSpec::Path(_)
        | LibSourceSpec::Url(_)
        | LibSourceSpec::Bytes(_)
        | LibSourceSpec::CratesIo(_) => None,
    }
}

fn push_unique_symbol(symbols: &mut Vec<Symbol>, symbol: Symbol) {
    if !symbols.iter().any(|existing| existing == &symbol) {
        symbols.push(symbol);
    }
}

impl Default for LoadSession {
    fn default() -> Self {
        Self::new()
    }
}

fn ensure_loadable_path(
    data_source: &KernelLibSourceSpec,
    source: &LibSourceSpec,
) -> Result<(), CliError> {
    if let KernelLibSourceSpec::Path(path) = data_source
        && !path.exists()
    {
        return Err(CliError::new(format!(
            "path source not found for {source}: {}",
            path.display()
        )));
    }
    Ok(())
}

fn load_error(source: &LibSourceSpec, err: KernelError) -> CliError {
    CliError::new(format!("load failed for {source}: {err}"))
}

fn no_codec_error(codec_name: &str, err: CliError) -> CliError {
    CliError::new(format!(
        "no codec '{codec_name}' available; provide one with --load ({err})"
    ))
}

fn manifest_exports_codec(manifest: &LibManifest, codec_symbol: &Symbol) -> bool {
    manifest.exports.iter().any(|export| match export {
        sim_kernel::Export::Codec { symbol, .. } => symbol == codec_symbol,
        _ => false,
    })
}

fn manifest_exports_requested_site(manifest: &LibManifest, sites: &[Symbol]) -> bool {
    manifest.exports.iter().any(|export| match export {
        sim_kernel::Export::Site { symbol, .. } => sites.iter().any(|site| site == symbol),
        _ => false,
    })
}

fn pre_site_config(boot: &CliBoot) -> crate::ConfigLoadOptions {
    let mut config = boot.config.clone();
    config.site_sources.clear();
    config
}
