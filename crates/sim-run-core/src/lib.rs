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

mod args;
mod boot;
mod bootloader;
mod codec_boot;
mod command;
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
mod platform_bundle;
mod receipt;
mod report;
mod source;

#[cfg(test)]
mod bootstrap_tests;
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
mod lib_tests;
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
pub use command::{
    CliError, run, run_command_with_session, run_command_with_session_at_version,
    run_supplied_bootstrap, run_with_session, version_line, version_line_for,
};
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
pub use envelope::cli_envelope_args;
#[cfg(feature = "registry")]
pub use git_registry::{GIT_REGISTRY_ENDPOINT_ENV, GitRegistryResolver};
pub use handoff::{CLI_MAIN_ENTRYPOINT, CliEntrypoint, cli_main_entrypoint_symbol};
pub use load::LoadSession;
pub use platform_bundle::{BootLoadRequest, boot_load_requests};
pub use receipt::{LoadReceipt, LoadReceiptRole};
pub use report::{
    ConfigReportKind, ConfigReportRequest, ConfigSourceReport, LoadedLibReport, LoadedStateReport,
    SourceStatus, format_config_sources, format_config_sources_json, format_config_status,
    format_config_status_json, format_effective_config, format_effective_config_json,
    render_config_report,
};
pub use sim_platform_bootstrap::{BootstrapEnvelope, BootstrappedCapsule};
pub use source::LibSourceSpec;
