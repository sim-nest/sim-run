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

mod glasses;
mod glasses_args;
mod glasses_plan;
mod index;
#[cfg(any(feature = "dynamic-native", feature = "wasm"))]
mod loader_boot;
mod watch;
mod watch_args;

fn main() {
    let code = boot().unwrap_or_else(|err| {
        eprintln!("sim: {err}");
        2
    });
    process::exit(code);
}

#[cfg(not(any(feature = "dynamic-native", feature = "wasm")))]
fn boot() -> Result<i32, sim_run_core::CliError> {
    let command = sim_run_core::parse_args(std::env::args_os())?;
    let mut session = watch::with_watch_if_selected(&command, sim_run_core::LoadSession::new());
    session = glasses::with_glasses_if_selected(&command, session);
    session = index::with_index_if_selected(&command, session);
    sim_run_core::run_command_with_session(command, &mut session)
}

#[cfg(any(feature = "dynamic-native", feature = "wasm"))]
fn boot() -> Result<i32, sim_run_core::CliError> {
    loader_boot::run(std::env::args_os())
}
