#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! The `sim` bootloader binary.
//!
//! Parses the boot envelope and hands off to a loaded library via
//! [`sim_run_core::run`]; all other behavior is provided by loaded libs. The
//! default build registers only the in-process host loader. Built
//! `--features dynamic-native`, it composes the native dynamic-library loader so
//! `sim --load path:lib.so` loads a real `.so`/`.dylib`/`.dll` plugin. Built
//! `--features wasm`, it composes the wasm loader so `sim --load
//! path:fixture.wasm` loads a portable plugin. Add the `registry` feature and
//! `SIM_GIT_REGISTRY_ENDPOINT` to resolve `symbol:` fallbacks from a git registry
//! artifact endpoint.

use std::process;

const TYPESCRIPT_NOTATION_HELP: &str =
    "Language profile: language/typescript-notation — TypeScript notation; does not type-check.\n";

mod boot_codec;
mod compute;
mod estate;
mod expr_tree;
mod glasses;
mod glasses_args;
mod glasses_plan;
mod index;
#[cfg(not(any(feature = "dynamic-native", feature = "wasm")))]
mod jvm;
#[cfg(any(feature = "dynamic-native", feature = "wasm"))]
mod loader_boot;
mod model_test;
mod physics;
mod platform;
mod provider;
mod relation;
mod roadmap;
mod search;
mod study;
mod watch;
mod watch_args;

/// Runs the complete product process adapter; binaries delegate here in one call.
pub fn process_main() {
    let envelope = sim_platform_ubuntu_pc::UbuntuProcessEnvelope::capture().unwrap_or_else(|err| {
        eprintln!("sim: capture process envelope: {err}");
        process::exit(2);
    });
    process_main_with(envelope);
}

/// Runs the bootloader from process facts supplied by a platform capsule.
pub fn process_main_with(envelope: sim_platform_ubuntu_pc::UbuntuProcessEnvelope) {
    if envelope
        .argv
        .iter()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        print!("{TYPESCRIPT_NOTATION_HELP}");
    }
    let code = boot(envelope).unwrap_or_else(|err| {
        eprintln!("sim: {err}");
        2
    });
    process::exit(code);
}

#[cfg(not(any(feature = "dynamic-native", feature = "wasm")))]
fn boot(
    envelope: sim_platform_ubuntu_pc::UbuntuProcessEnvelope,
) -> Result<i32, sim_run_core::CliError> {
    let command = sim_run_core::parse_args(envelope.argv)?;
    let cache = envelope
        .cache_root
        .unwrap_or_else(|| envelope.work_root.join(".sim/cache/libs"));
    let mut session =
        watch::with_watch_if_selected(&command, sim_run_core::LoadSession::with_cache_root(cache));
    session = glasses::with_glasses_if_selected(&command, session);
    session = index::with_index_if_selected(&command, session);
    session = provider::with_provider_if_selected(&command, session);
    session = relation::with_relation_if_selected(&command, session);
    session = roadmap::with_roadmap_if_selected(&command, session);
    session = search::with_search_if_selected(&command, session);
    session = model_test::with_model_test_if_selected(&command, session);
    session = platform::with_platform_if_selected(&command, session);
    session = physics::with_physics_if_selected(&command, session);
    session = jvm::with_jvm_if_selected(&command, session);
    session = compute::with_compute_if_selected(&command, session);
    session = estate::with_estate_if_selected(&command, session);
    session = expr_tree::with_expr_tree_if_selected(&command, session);
    session = study::with_study_if_selected(&command, session);
    sim_run_core::run_command_with_session_at_version(
        command,
        &mut session,
        env!("CARGO_PKG_VERSION"),
    )
}

#[cfg(any(feature = "dynamic-native", feature = "wasm"))]
fn boot(
    envelope: sim_platform_ubuntu_pc::UbuntuProcessEnvelope,
) -> Result<i32, sim_run_core::CliError> {
    loader_boot::run(envelope)
}
