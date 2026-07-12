//! Runtime configuration discovery for the bootloader.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use sim_codec_config::ConfigDecoder;
use sim_config::{
    ConfigDir, ConfigLayer, ConfigRoots, ConfigSource, ConfigTable, EffectiveConfig,
    lib_config_path, lib_symbol_from_str, merge_layers,
};
use sim_kernel::{Cx, Symbol};

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

    /// Returns non-fatal discovery diagnostics.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    /// Adds a layer and recomputes the effective config.
    pub fn push_layer(&mut self, layer: ConfigLayer) {
        self.layers.push(layer);
        self.effective = merge_layers(&self.layers);
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
    let mut state = RuntimeConfigState::default();
    let libs = unique_libs(libs);
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
    if !path.exists() {
        return;
    }
    let source = match kind {
        RootKind::Home => ConfigSource::HomeFile { path: path.clone() },
        RootKind::Work => ConfigSource::WorkFile { path: path.clone() },
    };
    let table = match decode_table_file(&path) {
        Ok(table) => table,
        Err(err) => {
            state.push_diagnostic(err);
            return;
        }
    };
    match ConfigDir::one(lib.clone(), table) {
        Ok(dir) => state.push_layer(ConfigLayer::new(source, dir)),
        Err(err) => state.push_diagnostic(format!("read config {}: {err}", path.display())),
    }
}

fn read_single_file(state: &mut RuntimeConfigState, path: &Path, explicit: bool) {
    if !path.exists() {
        if explicit {
            state.push_diagnostic(format!("config file not found: {}", path.display()));
        }
        return;
    }
    let dir = match decode_dir_file(path) {
        Ok(dir) => dir,
        Err(err) => {
            state.push_diagnostic(err);
            return;
        }
    };
    state.push_layer(ConfigLayer::new(
        ConfigSource::SingleFile {
            path: path.to_path_buf(),
        },
        dir,
    ));
}

fn read_site_dir(cx: &mut Cx, state: &mut RuntimeConfigState, site: &Symbol) {
    let Some(value) = cx.registry().site_by_symbol(site).cloned() else {
        state.push_diagnostic(format!("config site not found: {site}"));
        return;
    };
    let expr = match value.object().as_expr(cx) {
        Ok(expr) => expr,
        Err(err) => {
            state.push_diagnostic(format!("read config site {site}: {err}"));
            return;
        }
    };
    match ConfigDir::from_dir_expr(&expr).and_then(normalize_dir) {
        Ok(dir) => state.push_layer(ConfigLayer::new(
            ConfigSource::Site { site: site.clone() },
            dir,
        )),
        Err(err) => state.push_diagnostic(format!("read config site {site}: {err}")),
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
