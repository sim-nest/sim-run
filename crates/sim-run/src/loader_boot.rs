#[cfg(feature = "registry")]
use sim_run_core::CratesIoResolver;
use sim_run_core::{CliCommand, CliError, LoadSession};
use std::path::PathBuf;

pub(crate) fn run(
    envelope: sim_platform_ubuntu_pc::UbuntuProcessEnvelope,
) -> Result<i32, CliError> {
    let command = sim_run_core::parse_args(envelope.argv)?;
    let cache = envelope
        .cache_root
        .clone()
        .unwrap_or_else(|| envelope.work_root.join(".sim/cache/libs"));
    #[cfg(feature = "registry")]
    let endpoint = envelope.registry_endpoint;
    #[cfg(not(feature = "registry"))]
    let endpoint = None;
    #[cfg(feature = "registry")]
    let allow_insecure = envelope.allow_insecure_registry;
    #[cfg(not(feature = "registry"))]
    let allow_insecure = false;
    let mut session = loader_session(&command, cache, endpoint, allow_insecure)?;
    session = crate::watch::with_watch_if_selected(&command, session);
    session = crate::glasses::with_glasses_if_selected(&command, session);
    session = crate::index::with_index_if_selected(&command, session);
    session = crate::provider::with_provider_if_selected(&command, session);
    session = crate::platform::with_platform_if_selected(&command, session);
    session = crate::compute::with_compute_if_selected(&command, session);
    session = crate::expr_tree::with_expr_tree_if_selected(&command, session);
    sim_run_core::run_command_with_session_at_version(
        command,
        &mut session,
        env!("CARGO_PKG_VERSION"),
    )
}

fn loader_session(
    command: &CliCommand,
    cache: PathBuf,
    endpoint: Option<String>,
    allow_insecure: bool,
) -> Result<LoadSession, CliError> {
    let session = LoadSession::with_cache_root(cache);
    #[cfg(any(feature = "dynamic-native", feature = "wasm"))]
    let session = with_platform_loaders(session);
    #[cfg(feature = "registry")]
    let session = with_git_registry(session, endpoint, allow_insecure)?;
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
fn with_git_registry(
    session: LoadSession,
    endpoint: Option<String>,
    allow_insecure: bool,
) -> Result<LoadSession, CliError> {
    let Some(endpoint) = endpoint else {
        return Ok(session);
    };
    let resolver = CratesIoResolver::new(session.crates_io_cache_root().to_path_buf())
        .with_git_registry_endpoint_policy(endpoint, allow_insecure)?;
    Ok(session.with_crates_io_resolver(resolver))
}
