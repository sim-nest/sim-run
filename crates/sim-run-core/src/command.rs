use crate::{Bootloader, CliBoot, CliCommand, ConfigLoadOptions, LoadSession, Payload, parse_args};
use sim_platform_bootstrap::{BootstrapEnvelope, BootstrappedCapsule};
use std::{ffi::OsString, fmt};

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

/// Returns the version line for the `sim-run-core` package.
///
/// Product binaries whose package version can differ from the core crate use
/// [`version_line_for`] through [`run_command_with_session_at_version`].
pub fn version_line() -> String {
    version_line_for(env!("CARGO_PKG_VERSION"))
}

/// Returns the `sim --version` line for the binary-owned `version`.
pub fn version_line_for(version: &str) -> String {
    format!("sim {version}\n")
}

/// Runs the command entry API with process arguments.
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

/// Runs from a host-supplied bootstrap envelope and already-admitted capsule.
///
/// This entrypoint performs no argv, current-directory, environment, config,
/// target, or registry discovery. The capsule was resolved by the bounded
/// platform rind; this pure consumer only installs it and transfers owned
/// envelope frames into the existing loaded-library handoff.
///
/// # Errors
/// Returns an error when the supplied capsule cannot be installed, standard
/// input is not textual, or the selected loaded entrypoint rejects the boot.
pub fn run_supplied_bootstrap(
    bootstrapped: BootstrappedCapsule,
    session: &mut LoadSession,
) -> Result<i32, CliError> {
    use sim_config::ConfigRoots;

    let BootstrapEnvelope {
        argv,
        stdio,
        bundle_identity: _,
        capsule_card: _,
        preopened_roots,
        config_roots,
        kernel_seed: _,
    } = bootstrapped.envelope;
    session.install_supplied_capsule(bootstrapped.capsule)?;
    let mut args = argv;
    if !args.is_empty() {
        args.remove(0);
    }
    let stdin = if stdio.stdin.is_empty() {
        None
    } else {
        Some(
            String::from_utf8(stdio.stdin)
                .map_err(|_| CliError::new("bootstrap stdin is not valid UTF-8"))?,
        )
    };
    let _supplied_mounts = preopened_roots;
    let boot = CliBoot {
        codec: None,
        loads: Vec::new(),
        native_audio_provider: None,
        config: ConfigLoadOptions {
            roots: ConfigRoots::new(config_roots.home, config_roots.work),
            read_files: false,
            single_file: None,
            site_sources: Vec::new(),
        },
        list: false,
        inspect: None,
        config_report: None,
        payload: Payload {
            args,
            eval: None,
            script: None,
            stdin,
        },
    };
    session.run_loaded_boot(&boot)
}

/// Runs an already-parsed command with an injected loader session.
pub fn run_command_with_session(
    command: CliCommand,
    session: &mut LoadSession,
) -> Result<i32, CliError> {
    run_command_with_session_at_version(command, session, env!("CARGO_PKG_VERSION"))
}

/// Runs an already-parsed command using the product binary's own `version`.
pub fn run_command_with_session_at_version(
    command: CliCommand,
    session: &mut LoadSession,
    version: &str,
) -> Result<i32, CliError> {
    match command {
        CliCommand::Help => {
            print!("{HELP}");
            Ok(0)
        }
        CliCommand::Version => {
            print!("{}", version_line_for(version));
            Ok(0)
        }
        CliCommand::Boot(boot) => session.run_loaded_boot(&boot),
    }
}
