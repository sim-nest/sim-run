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
    session = crate::search::with_search_if_selected(&command, session);
    session = crate::world::with_world_if_selected(&command, session);
    session = crate::roadmap::with_roadmap_if_selected(&command, session);
    session = crate::platform::with_platform_if_selected(&command, session);
    session = crate::physics::with_physics_if_selected(&command, session);
    session = crate::compute::with_compute_if_selected(&command, session);
    session = crate::continuity::with_continuity_if_selected(&command, session);
    session = crate::estate::with_estate_if_selected(&command, session);
    session = crate::expr_tree::with_expr_tree_if_selected(&command, session);
    session = crate::model_test::with_model_test_if_selected(&command, session);
    session = crate::relation::with_relation_if_selected(&command, session);
    #[cfg(feature = "dynamic-native")]
    {
        session = crate::repl::with_repl_if_selected(&command, session);
    }
    session = crate::study::with_study_if_selected(&command, session);
    sim_run_core::run_command_with_session_at_version(
        command,
        &mut session,
        env!("CARGO_PKG_VERSION"),
    )
}

fn loader_session(
    command: &CliCommand,
    cache: PathBuf,
    _endpoint: Option<String>,
    _allow_insecure: bool,
) -> Result<LoadSession, CliError> {
    let session = LoadSession::with_cache_root(cache);
    #[cfg(any(
        feature = "wasm",
        all(feature = "dynamic-native", not(target_arch = "wasm32"))
    ))]
    let session = with_platform_loaders(session);
    #[cfg(feature = "registry")]
    let session = with_git_registry(session, _endpoint, _allow_insecure)?;
    let _ = command;
    Ok(session)
}

#[cfg(any(
    feature = "wasm",
    all(feature = "dynamic-native", not(target_arch = "wasm32"))
))]
fn with_platform_loaders(session: LoadSession) -> LoadSession {
    use std::sync::Arc;
    let port: Arc<dyn sim_run_loaders::LoaderPort> =
        Arc::new(sim_platform_ubuntu_pc::UbuntuLoaderPort::default());
    #[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
    let session = session
        .with_loader(sim_run_loaders::PortLoader::new(
            Arc::clone(&port),
            sim_run_loaders::LoaderKind::new(sim_kernel::Symbol::qualified("loader", "native-v1")),
            accepts_native_source,
        ))
        .with_capability(sim_kernel::native_dynamic_load_capability());
    #[cfg(feature = "wasm")]
    let session = session
        .with_loader(sim_run_loaders::PortLoader::new(
            port,
            sim_run_loaders::LoaderKind::new(sim_kernel::Symbol::qualified("loader", "wasm-v1")),
            accepts_wasm_source,
        ))
        .with_capability(sim_run_loaders::wasm_load_capability());
    session
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn accepts_native_source(source: &sim_kernel::LibSource) -> bool {
    sim_run_loaders::path_from_source(source).is_ok_and(|path| {
        path.is_some_and(|path| {
            matches!(
                path.extension().and_then(std::ffi::OsStr::to_str),
                Some("so" | "dylib" | "dll")
            )
        })
    })
}

#[cfg(feature = "wasm")]
fn accepts_wasm_source(source: &sim_kernel::LibSource) -> bool {
    sim_run_loaders::path_from_source(source).is_ok_and(|path| {
        path.is_some_and(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wasm")
        })
    }) || sim_run_loaders::bytes_from_source(source)
        .is_ok_and(|bytes| bytes.is_some_and(|bytes| bytes.starts_with(b"\0asm")))
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

#[cfg(all(
    test,
    feature = "wasm",
    feature = "dynamic-native",
    not(target_arch = "wasm32")
))]
mod tests {
    use super::{accepts_native_source, accepts_wasm_source};

    #[test]
    fn platform_loader_routes_only_exact_artifact_kinds() {
        let native = sim_run_loaders::path_source("fixture.so");
        let wasm_path = sim_run_loaders::path_source("fixture.wasm");
        let wasm_bytes = sim_run_loaders::bytes_source(b"\0asmfixture");
        let arbitrary_bytes = sim_run_loaders::bytes_source(b"not wasm");

        assert!(accepts_native_source(&native));
        assert!(!accepts_wasm_source(&native));
        assert!(accepts_wasm_source(&wasm_path));
        assert!(!accepts_native_source(&wasm_path));
        assert!(accepts_wasm_source(&wasm_bytes));
        assert!(!accepts_native_source(&wasm_bytes));
        assert!(!accepts_wasm_source(&arbitrary_bytes));
    }
}
