use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "model-test";
const HOST: &str = "lib/model-test";

pub(crate) fn with_model_test_if_selected(
    command: &CliCommand,
    session: LoadSession,
) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || Box::new(sim_lib_model_test::product::ModelTestLib))
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(HOST.into()),
            ],
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_test_is_bootloader_selected() {
        let command = sim_run_core::parse_args(["sim", "model-test", "status"]).unwrap();
        let CliCommand::Boot(boot) = command else {
            panic!("not boot command")
        };
        assert_eq!(boot.payload.args[0], "model-test");
    }
}
