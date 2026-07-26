use sim_kernel::{
    AbiVersion, Export, Lib, LibManifest, LibTarget, Linker, LoadCx, Result, Symbol, Version,
};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const COMPUTE_VERB: &str = "compute";
const COMPUTE_AUTO_HOST: &str = "compute/auto-lib";
const COMPUTE_CLI_HOST: &str = "lib/compute-cli";
const COMPUTE_MODEL_HOST: &str = "compute/model-lib";
const COMPUTE_BOOT_CODEC_HOST: &str = "codec/lisp";

pub(crate) fn with_compute_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_compute_command(command) {
        return session;
    }
    session
        .with_host_factory(COMPUTE_BOOT_CODEC_HOST, || Box::new(ComputeBootCodec))
        .with_host_factory(COMPUTE_MODEL_HOST, || {
            Box::new(sim_lib_compute_model::ComputeModelLib::default())
        })
        .with_host_factory(COMPUTE_AUTO_HOST, || {
            Box::new(sim_lib_compute_auto::ComputeAutoLib::default())
        })
        .with_host_factory(COMPUTE_CLI_HOST, || {
            Box::new(sim_lib_compute_cli::ComputeCliLib::new())
        })
        .with_capability(sim_lib_compute_cli::compute_device_capability())
        .with_capability(sim_lib_compute_cli::compute_profile_read_capability())
        .with_capability(sim_lib_compute_cli::compute_profile_write_capability())
        .with_default_verb_sources(
            COMPUTE_VERB,
            vec![
                LibSourceSpec::Host(COMPUTE_BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(COMPUTE_MODEL_HOST.to_owned()),
                LibSourceSpec::Host(COMPUTE_AUTO_HOST.to_owned()),
                LibSourceSpec::Host(COMPUTE_CLI_HOST.to_owned()),
            ],
        )
}

fn is_compute_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == COMPUTE_VERB)
}

struct ComputeBootCodec;

impl Lib for ComputeBootCodec {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("codec", "lisp"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Codec {
                symbol: Symbol::qualified("codec", "lisp"),
                codec_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.codec_value(Symbol::qualified("codec", "lisp"), cx.factory().bool(true)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sim_run_core::parse_args;

    use super::is_compute_command;

    #[test]
    fn detects_compute_payload_verb() {
        let command = parse_args(["sim", "compute", "devices"]).unwrap();
        assert!(is_compute_command(&command));
    }

    #[test]
    fn non_compute_payload_stays_on_default_boot_path() {
        let command = parse_args(["sim", "run"]).unwrap();
        assert!(!is_compute_command(&command));
    }
}
