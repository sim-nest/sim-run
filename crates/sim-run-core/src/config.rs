//! Runtime configuration discovery for the bootloader.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use sim_codec_config::ConfigDecoder;
use sim_config::{
    ConfigDir, ConfigLayer, ConfigProbe, ConfigProbeCaps, ConfigProbeReport, ConfigProbeRequest,
    ConfigRoots, ConfigSecretField, ConfigSource, ConfigTable, EffectiveConfig, ProbeMode,
    lib_config_path, lib_symbol_from_str, merge_layers,
};
use sim_kernel::{Cx, Symbol};

use crate::report::{ConfigSourceReport, SourceStatus};

/// Source-selection options for runtime configuration discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLoadOptions {
    /// Home and working config roots.
    pub roots: ConfigRoots,
    /// Whether filesystem config roots should be read.
    pub read_files: bool,
    /// Explicit shared config file to read after root files.
    pub single_file: Option<PathBuf>,
    /// Site exports that produce config Dir expressions.
    pub site_sources: Vec<Symbol>,
}

impl ConfigLoadOptions {
    /// Builds options from explicit roots.
    pub fn with_roots(roots: ConfigRoots) -> Self {
        Self {
            roots,
            read_files: true,
            single_file: None,
            site_sources: Vec::new(),
        }
    }
}

impl Default for ConfigLoadOptions {
    fn default() -> Self {
        let work_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_roots(ConfigRoots::from_env(work_root))
    }
}

/// Configuration layers discovered during a boot session.
#[derive(Clone, Debug, Default)]
pub struct RuntimeConfigState {
    layers: Vec<ConfigLayer>,
    effective: EffectiveConfig,
    source_reports: Vec<ConfigSourceReport>,
    secret_fields: Vec<ConfigSecretField>,
    probe_reports: Vec<ConfigProbeReport>,
    diagnostics: Vec<String>,
}

impl RuntimeConfigState {
    /// Returns the discovered layers in merge order.
    pub fn layers(&self) -> &[ConfigLayer] {
        &self.layers
    }

    /// Returns the effective config after all discovered layers are merged.
    pub fn effective(&self) -> &EffectiveConfig {
        &self.effective
    }

    /// Returns source status records in discovery order.
    pub fn source_reports(&self) -> &[ConfigSourceReport] {
        &self.source_reports
    }

    /// Returns config fields that must be redacted in reports.
    pub fn secret_fields(&self) -> &[ConfigSecretField] {
        &self.secret_fields
    }

    /// Returns typed probe report records in discovery order.
    pub fn probe_reports(&self) -> &[ConfigProbeReport] {
        &self.probe_reports
    }

    /// Returns non-fatal discovery diagnostics.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// Adds a layer and recomputes the effective config.
    pub fn push_layer(&mut self, layer: ConfigLayer) {
        self.push_source_report(layer.source.clone(), SourceStatus::Found);
        self.layers.push(layer);
        self.effective = merge_layers(&self.layers);
    }

    /// Adds or replaces shape-derived secret field metadata.
    pub fn extend_secret_fields(&mut self, fields: impl IntoIterator<Item = ConfigSecretField>) {
        for field in fields {
            if !self.secret_fields.contains(&field) {
                self.secret_fields.push(field);
            }
        }
    }

    /// Adds a typed probe report record.
    pub fn push_probe_report(&mut self, report: ConfigProbeReport) {
        self.probe_reports.push(report);
    }

    /// Adds a source status record.
    pub fn push_source_report(&mut self, source: ConfigSource, status: SourceStatus) {
        self.source_reports
            .push(ConfigSourceReport { source, status });
    }

    fn push_diagnostic(&mut self, diagnostic: String) {
        self.diagnostics.push(diagnostic);
    }
}

/// Loads config layers from files and site exports.
pub fn load_config_sources(
    cx: &mut Cx,
    opts: &ConfigLoadOptions,
    libs: &[Symbol],
) -> RuntimeConfigState {
    load_config_sources_with_probes(cx, opts, libs, &[])
}

/// Loads config layers from probes, files, and site exports.
pub fn load_config_sources_with_probes(
    cx: &mut Cx,
    opts: &ConfigLoadOptions,
    libs: &[Symbol],
    probes: &[&dyn ConfigProbe],
) -> RuntimeConfigState {
    let mut state = RuntimeConfigState::default();
    let libs = unique_libs(libs);
    run_config_probes(
        &mut state,
        &libs,
        probes,
        ProbeMode::default(),
        ConfigProbeCaps::default(),
    );
    if opts.read_files {
        read_root_files(&mut state, &opts.roots.home, RootKind::Home, &libs);
        read_root_files(
            &mut state,
            &Some(opts.roots.work.clone()),
            RootKind::Work,
            &libs,
        );
        if let Some(path) = opts.single_file.as_ref() {
            read_single_file(&mut state, path, true);
        }
    }
    for site in &opts.site_sources {
        read_site_dir(cx, &mut state, site);
    }
    state
}

fn run_config_probes(
    state: &mut RuntimeConfigState,
    libs: &[Symbol],
    probes: &[&dyn ConfigProbe],
    mode: ProbeMode,
    caps: ConfigProbeCaps,
) {
    for lib in libs {
        for probe in probes {
            let request = ConfigProbeRequest {
                lib: lib.clone(),
                mode,
                caps: caps.clone(),
            };
            run_config_probe(state, *probe, &request);
        }
    }
}

/// Executes one config probe, applying any emitted layer and recording its report.
pub fn run_config_probe(
    state: &mut RuntimeConfigState,
    probe: &dyn ConfigProbe,
    request: &ConfigProbeRequest,
) {
    let (layer, report) = probe.probe(request);
    if let Some(layer) = layer {
        state.push_layer(layer);
    }
    state.push_probe_report(report);
}

fn unique_libs(libs: &[Symbol]) -> Vec<Symbol> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for lib in libs {
        if seen.insert(lib.clone()) {
            unique.push(lib.clone());
        }
    }
    unique
}

#[derive(Clone, Copy)]
enum RootKind {
    Home,
    Work,
}

fn read_root_files(
    state: &mut RuntimeConfigState,
    root: &Option<PathBuf>,
    kind: RootKind,
    libs: &[Symbol],
) {
    let Some(root) = root.as_ref() else {
        return;
    };
    for lib in libs {
        read_per_lib_file(state, root, kind, lib);
    }
    read_single_file(state, &root.join("sim.toml"), false);
}

fn read_per_lib_file(state: &mut RuntimeConfigState, root: &Path, kind: RootKind, lib: &Symbol) {
    let relative = match lib_config_path(lib) {
        Ok(relative) => relative,
        Err(err) => {
            state.push_diagnostic(format!("skip config path for {lib}: {err}"));
            return;
        }
    };
    let path = root.join(relative);
    let source = match kind {
        RootKind::Home => ConfigSource::HomeFile { path: path.clone() },
        RootKind::Work => ConfigSource::WorkFile { path: path.clone() },
    };
    if !path.exists() {
        state.push_source_report(source, SourceStatus::Missing);
        return;
    }
    let table = match decode_table_file(&path) {
        Ok(table) => table,
        Err(err) => {
            state.push_source_report(source, SourceStatus::Rejected);
            state.push_diagnostic(err);
            return;
        }
    };
    match ConfigDir::one(lib.clone(), table) {
        Ok(dir) => state.push_layer(ConfigLayer::new(source, dir)),
        Err(err) => {
            state.push_source_report(source, SourceStatus::Rejected);
            state.push_diagnostic(format!("read config {}: {err}", path.display()));
        }
    }
}

fn read_single_file(state: &mut RuntimeConfigState, path: &Path, explicit: bool) {
    let source = ConfigSource::SingleFile {
        path: path.to_path_buf(),
    };
    if !path.exists() {
        state.push_source_report(source, SourceStatus::Missing);
        if explicit {
            state.push_diagnostic(format!("config file not found: {}", path.display()));
        }
        return;
    }
    let dir = match decode_dir_file(path) {
        Ok(dir) => dir,
        Err(err) => {
            state.push_source_report(source, SourceStatus::Rejected);
            state.push_diagnostic(err);
            return;
        }
    };
    state.push_layer(ConfigLayer::new(source, dir));
}

fn read_site_dir(cx: &mut Cx, state: &mut RuntimeConfigState, site: &Symbol) {
    let source = ConfigSource::Site { site: site.clone() };
    let Some(value) = cx.registry().site_by_symbol(site).cloned() else {
        state.push_source_report(source, SourceStatus::Missing);
        state.push_diagnostic(format!("config site not found: {site}"));
        return;
    };
    let expr = match value.object().as_expr(cx) {
        Ok(expr) => expr,
        Err(err) => {
            state.push_source_report(source, SourceStatus::Rejected);
            state.push_diagnostic(format!("read config site {site}: {err}"));
            return;
        }
    };
    match ConfigDir::from_dir_expr(&expr).and_then(normalize_dir) {
        Ok(dir) => state.push_layer(ConfigLayer::new(source, dir)),
        Err(err) => {
            state.push_source_report(source, SourceStatus::Rejected);
            state.push_diagnostic(format!("read config site {site}: {err}"));
        }
    }
}

fn decode_table_file(path: &Path) -> Result<sim_kernel::Expr, String> {
    let source = read_ascii(path)?;
    ConfigDecoder::table()
        .decode_text(&source)
        .map_err(|err| format!("decode config table {}: {err}", path.display()))
}

fn decode_dir_file(path: &Path) -> Result<ConfigDir, String> {
    let source = read_ascii(path)?;
    let expr = ConfigDecoder::dir()
        .decode_text(&source)
        .map_err(|err| format!("decode config dir {}: {err}", path.display()))?;
    ConfigDir::from_dir_expr(&expr)
        .and_then(normalize_dir)
        .map_err(|err| format!("decode config dir {}: {err}", path.display()))
}

fn normalize_dir(dir: ConfigDir) -> sim_config::ConfigResult<ConfigDir> {
    let mut normalized = ConfigDir::new();
    for table in dir.entries {
        let lib = lib_symbol_from_str(&table.lib.as_qualified_str())?;
        normalized.upsert(ConfigTable::new(lib, table.table)?);
    }
    Ok(normalized)
}

fn read_ascii(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("read config {}: {err}", path.display()))
}
