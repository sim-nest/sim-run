use std::ffi::OsString;
#[cfg(feature = "wasm")]
use std::sync::Arc;

#[cfg(feature = "dynamic-native")]
use sim_run_core::{CliBoot, LibSourceSpec};
use sim_run_core::{CliCommand, CliError, LoadSession};
#[cfg(feature = "registry")]
use sim_run_core::{CratesIoResolver, GIT_REGISTRY_ENDPOINT_ENV};

#[cfg(feature = "dynamic-native")]
use std::{env, path::PathBuf};

#[cfg(feature = "dynamic-native")]
const REPL_BUNDLE_DIR_ENV: &str = "SIM_REPL_BUNDLE_DIR";

#[cfg(feature = "dynamic-native")]
struct ReplBundle {
    codec_lisp: PathBuf,
    numbers_f64: PathBuf,
    standard_core: PathBuf,
}

pub(crate) fn run<I, S>(args: I) -> Result<i32, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let command = sim_run_core::parse_args(args)?;
    let mut session = loader_session(&command)?;
    session = crate::watch::with_watch_if_selected(&command, session);
    sim_run_core::run_command_with_session(command, &mut session)
}

fn loader_session(command: &CliCommand) -> Result<LoadSession, CliError> {
    let session = LoadSession::new();
    #[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
    let session = with_native_loader(session);
    #[cfg(feature = "wasm")]
    let session = with_wasm_loader(session);
    #[cfg(feature = "registry")]
    let session = with_git_registry(session)?;
    #[cfg(not(feature = "dynamic-native"))]
    let _ = command;
    #[cfg(feature = "dynamic-native")]
    match command {
        CliCommand::Boot(boot) if uses_default_repl_bundle(boot) => repl_session(session),
        _ => Ok(session),
    }
    #[cfg(not(feature = "dynamic-native"))]
    {
        Ok(session)
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn with_native_loader(session: LoadSession) -> LoadSession {
    session
        .with_loader(sim_run_loaders::NativeDylibLoader)
        .with_capability(sim_kernel::native_dynamic_load_capability())
}

#[cfg(feature = "wasm")]
fn with_wasm_loader(session: LoadSession) -> LoadSession {
    session
        .with_loader(sim_run_loaders::WasmLoader::new(Arc::new(
            sim_wasm_abi::WasmiRuntime::new(),
        )))
        .with_capability(sim_run_loaders::wasm_load_capability())
}

#[cfg(feature = "registry")]
fn with_git_registry(session: LoadSession) -> Result<LoadSession, CliError> {
    let Some(endpoint) = std::env::var_os(GIT_REGISTRY_ENDPOINT_ENV) else {
        return Ok(session);
    };
    let resolver = CratesIoResolver::default()
        .with_git_registry_endpoint(endpoint.to_string_lossy().into_owned())?;
    Ok(session.with_crates_io_resolver(resolver))
}

#[cfg(feature = "dynamic-native")]
fn repl_session(session: LoadSession) -> Result<LoadSession, CliError> {
    let bundle = ReplBundle::resolve()?;
    Ok(session
        .with_host_factory("lib/repl", || Box::new(sim_lib_repl::ReplLib::new()))
        .with_capability(sim_kernel::macro_expand_capability())
        .with_capability(sim_kernel::macro_expand_eval_capability())
        .with_default_verb_sources(
            "repl",
            vec![
                LibSourceSpec::Path(bundle.codec_lisp),
                LibSourceSpec::Path(bundle.numbers_f64),
                LibSourceSpec::Path(bundle.standard_core),
                LibSourceSpec::Host("lib/repl".to_owned()),
            ],
        ))
}

#[cfg(feature = "dynamic-native")]
fn uses_default_repl_bundle(boot: &CliBoot) -> bool {
    boot.loads.is_empty()
        && (boot.payload.eval.is_some()
            || boot
                .payload
                .args
                .first()
                .is_some_and(|arg| matches!(arg.to_string_lossy().as_ref(), "repl" | "eval")))
}

#[cfg(feature = "dynamic-native")]
impl ReplBundle {
    fn resolve() -> Result<Self, CliError> {
        let dirs = candidate_bundle_dirs();
        let codec_lisp = find_required_dylib(&dirs, "sim_codec_lisp")?;
        let numbers_f64 = find_required_dylib(&dirs, "sim_lib_numbers_f64")?;
        let standard_core = find_required_dylib(&dirs, "sim_lib_standard_core")?;
        Ok(Self {
            codec_lisp,
            numbers_f64,
            standard_core,
        })
    }
}

#[cfg(feature = "dynamic-native")]
fn candidate_bundle_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = env::var_os(REPL_BUNDLE_DIR_ENV) {
        dirs.push(PathBuf::from(path));
    }
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        push_unique(&mut dirs, parent.to_path_buf());
        if parent.file_name().is_some_and(|name| name == "deps")
            && let Some(debug_dir) = parent.parent()
        {
            push_unique(&mut dirs, debug_dir.to_path_buf());
        }
    }
    dirs
}

#[cfg(feature = "dynamic-native")]
fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.iter().any(|candidate| candidate == &dir) {
        dirs.push(dir);
    }
}

#[cfg(feature = "dynamic-native")]
fn find_required_dylib(dirs: &[PathBuf], stem: &str) -> Result<PathBuf, CliError> {
    let name = dylib_file_name(stem);
    dirs.iter()
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| missing_bundle_error(&name, dirs))
}

#[cfg(feature = "dynamic-native")]
fn missing_bundle_error(name: &str, dirs: &[PathBuf]) -> CliError {
    let searched = if dirs.is_empty() {
        "<none>".to_owned()
    } else {
        dirs.iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    CliError::new(format!(
        "repl native bundle is missing {name}; searched {searched}. Build the native dylib bundle into the sim binary target directory or set {REPL_BUNDLE_DIR_ENV} to the directory containing the required native dylibs"
    ))
}

#[cfg(feature = "dynamic-native")]
fn dylib_file_name(stem: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("{stem}.dll")
    }
    #[cfg(target_os = "macos")]
    {
        format!("lib{stem}.dylib")
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        format!("lib{stem}.so")
    }
}
