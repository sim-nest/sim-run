#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Core command entry API for the SIM bootloader.
//!
//! # Bootloader frame
//!
//! The shipped `sim` binary is a **bootloader frame, not a batteries-included
//! runtime**. [`run`] builds a [`LoadSession`] whose only registered loader is
//! the in-process [`LoadSession::add_host_factory`] host loader: with no host
//! factory and no injected artifact loader it can boot **no codec and no
//! library**, so `run(["sim", "run"])` fails with `no codec 'lisp' available`.
//! This is by design: behavior lives in loadable libraries, not baked into the
//! frame. The default frame loads a codec when an explicit source, cache
//! artifact, registry resolver, or host factory supplies it.
//!
//! A working session therefore comes from one of:
//!
//! - an explicitly provided source: `--load path/to/artifact.simlib` (needs an
//!   artifact loader registered via [`LoadSession::with_loader`]), or
//! - a seeded cache resolved by the cache-only [`CratesIoResolver`] (it never
//!   reaches the network unless an explicit registry resolver is installed; the
//!   cache must otherwise already hold the artifact), or
//! - a host factory registered through [`LoadSession::with_host_factory`] and
//!   driven via [`run_with_session`] -- the path every functional test uses.
//!
//! The `registry` feature adds a git registry artifact resolver, but it is active
//! only when the host installs it. Nothing here bakes in a codec.

use std::{ffi::OsString, fmt};

mod args;
mod boot;
mod bootloader;
mod codec_boot;
mod config;
mod crates_io;
mod device_host;
pub mod device_options;
mod envelope;
mod exit;
#[cfg(feature = "registry")]
mod git_registry;
mod handoff;
mod host;
mod introspect;
mod load;
mod receipt;
mod report;
mod source;

#[cfg(test)]
mod codec_boot_tests;
#[cfg(test)]
mod config_report_tests;
#[cfg(test)]
mod config_site_tests;
#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod device_host_tests;
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod introspect_tests;
#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod publish_tests;
#[cfg(test)]
mod scenario_tests;

pub use args::{CliCommand, parse_args};
pub use boot::{CliBoot, CliEnvelope, Payload};
pub use bootloader::Bootloader;
pub use codec_boot::{DEFAULT_CODEC_NAME, boot_codec_name, codec_lib_symbol};
pub use config::{
    ConfigLoadOptions, RuntimeConfigState, load_config_sources, load_config_sources_with_probes,
    run_config_probe,
};
pub use crates_io::{CratesIoResolver, CratesIoSpec, ResolvedCratesIoSource, VersionReq};
pub use device_host::{
    AdapterTick, DeviceAdapterLoopPlan, DeviceConsentPolicy, DeviceEdgeSession, DeviceHostSpec,
    DeviceHostStalePolicy, DevicePlacement, DevicePlacementError, DeviceProfile, DeviceProvider,
    DeviceProviderKind, DeviceRateClass, DeviceSession, DeviceSite, DeviceSiteLocality,
    DeviceSurfaceHubJoin, RouteArg, StubProvider, StubSession, compose_device_host,
    compose_device_host_with_provider, derive_device_rate_class, install_device_bases,
};
#[cfg(feature = "registry")]
pub use git_registry::{GIT_REGISTRY_ENDPOINT_ENV, GitRegistryResolver};
pub use handoff::{CLI_MAIN_ENTRYPOINT, CliEntrypoint, cli_main_entrypoint_symbol};
pub use load::LoadSession;
pub use receipt::{LoadReceipt, LoadReceiptRole};
pub use report::{
    ConfigReportKind, ConfigReportRequest, ConfigSourceReport, LoadedLibReport, LoadedStateReport,
    SourceStatus, format_config_sources, format_config_sources_json, format_config_status,
    format_config_status_json, format_effective_config, format_effective_config_json,
    render_config_report,
};
pub use source::LibSourceSpec;

const HELP: &str = "\
Usage: sim [OPTIONS] [PAYLOAD...]

Options:
  --help              Print this help text.
  --version           Print the binary version.
  --codec NAME        Select the boot codec name.
  --load SRC          Add a library source to load.
  --native-audio-provider SRC
                      Try a native audio provider source and degrade if absent.
  --config-home PATH  Read home config from PATH.
  --config-work PATH  Read working config from PATH.
  --config-file PATH  Read one shared config Dir file after root files.
  --config-site SYMBOL
                      Read a config Dir from a loaded site export.
  --no-config-files   Skip filesystem config discovery.
  --list              Request a loaded-lib list.
  --inspect SYMBOL    Request inspection of a loaded lib or export.
  config status       Report loaded libs, config sources, probes, and diagnostics.
  config effective LIB
                      Report the effective config table for LIB.
  config sources      Report config source provenance and diagnostics.
  --json              Render a config report command as stable JSON.
  --eval TEXT         Carry eval text for loaded-lib handoff.
  --script PATH       Carry a script path for loaded-lib handoff.
  --stdin TEXT        Carry stdin text for loaded-lib handoff.

Note: the bootloader bakes in no codec. By default it fetches nothing over
the network and boots only libraries provided via --load (an artifact source) or
already present in the local cache. A build with the registry feature can fetch from
an explicit git registry endpoint installed by the host. With no source it reports
`no codec '<name>' available`.
";

/// Command-line error returned by the bootloader core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
}

impl CliError {
    /// Builds a command-line error from a user-facing message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn unsupported(arg: &str) -> Self {
        Self::new(format!("unsupported argument: {arg}"))
    }

    pub(crate) fn missing_value(flag: &str) -> Self {
        Self::new(format!("{flag} requires a value"))
    }

    pub(crate) fn duplicate(flag: &str) -> Self {
        Self::new(format!("{flag} was provided more than once"))
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// Returns the version line printed by `sim --version`.
pub fn version_line() -> String {
    format!("sim {}\n", env!("CARGO_PKG_VERSION"))
}

/// Runs the command entry API with process arguments.
///
/// This is the one public boot path, expressed through [`Bootloader`]: the default
/// `sim` runtime is `Bootloader::standard()` (the in-process host loader only), so it
/// boots no codec or library unless a loadable source is supplied via `--load` or
/// already cached. A `Boot` command with no available codec returns
/// `no codec '<name>' available`. To boot a real codec or library in-process, compose
/// a [`Bootloader`] with [`Bootloader::host_verb`]/[`Bootloader::host_lib`] (or build a
/// session with [`LoadSession::with_host_factory`]/[`LoadSession::with_loader`] and
/// call [`run_with_session`]).
pub fn run<I, S>(args: I) -> Result<i32, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    Bootloader::standard().run(args)
}

/// Runs the command entry API with an injected loader session.
pub fn run_with_session<I, S>(args: I, session: &mut LoadSession) -> Result<i32, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    run_command_with_session(parse_args(args)?, session)
}

/// Runs an already-parsed command with an injected loader session.
pub fn run_command_with_session(
    command: CliCommand,
    session: &mut LoadSession,
) -> Result<i32, CliError> {
    match command {
        CliCommand::Help => {
            print!("{HELP}");
            Ok(0)
        }
        CliCommand::Version => {
            print!("{}", version_line());
            Ok(0)
        }
        CliCommand::Boot(boot) => session.run_loaded_boot(&boot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_uses_package_version() {
        assert_eq!(
            version_line(),
            format!("sim {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn direct_payload_enters_loaded_boot() {
        let err = run(["sim", "run"]).unwrap_err();
        assert!(err.to_string().starts_with("no codec 'lisp' available"));
    }
}
