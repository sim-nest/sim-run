use sim_run_core::{CliCommand, CliError, LoadSession};
#[cfg(feature = "registry")]
use sim_run_core::{CratesIoResolver, GIT_REGISTRY_ENDPOINT_ENV};
use std::ffi::OsString;

pub(crate) fn run<I, S>(args: I) -> Result<i32, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let command = sim_run_core::parse_args(args)?;
    let mut session = loader_session(&command)?;
    session = crate::watch::with_watch_if_selected(&command, session);
    session = crate::glasses::with_glasses_if_selected(&command, session);
    session = crate::index::with_index_if_selected(&command, session);
    session = crate::compute::with_compute_if_selected(&command, session);
    session = crate::expr_tree::with_expr_tree_if_selected(&command, session);
    sim_run_core::run_command_with_session_at_version(
        command,
        &mut session,
        env!("CARGO_PKG_VERSION"),
    )
}

fn loader_session(command: &CliCommand) -> Result<LoadSession, CliError> {
    let session = LoadSession::new();
    #[cfg(any(feature = "dynamic-native", feature = "wasm"))]
    let session = with_platform_loaders(session);
    #[cfg(feature = "registry")]
    let session = with_git_registry(session)?;
    let _ = command;
    Ok(session)
}

#[cfg(any(feature = "dynamic-native", feature = "wasm"))]
fn with_platform_loaders(session: LoadSession) -> LoadSession {
    use std::sync::Arc;
    let port: Arc<dyn sim_run_loaders::LoaderPort> =
        Arc::new(sim_platform_ubuntu_pc::UbuntuLoaderPort::default());
    session
        .with_loader(sim_run_loaders::PortLoader::new(
            Arc::clone(&port),
            sim_run_loaders::LoaderKind::new(sim_kernel::Symbol::qualified("loader", "native-v1")),
            sim_run_loaders::is_path_source,
        ))
        .with_loader(sim_run_loaders::PortLoader::new(
            port,
            sim_run_loaders::LoaderKind::new(sim_kernel::Symbol::qualified("loader", "wasm-v1")),
            sim_run_loaders::is_bytes_source,
        ))
        .with_capability(sim_kernel::native_dynamic_load_capability())
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
