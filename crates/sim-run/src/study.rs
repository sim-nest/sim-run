use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "study";
const HOST: &str = "lib/study";

pub(crate) fn with_study_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || Box::new(sim_lib_study::product::StudyLib))
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
    fn study_is_a_bootloader_selected_library() {
        let command =
            sim_run_core::parse_args(["sim", "study", "status", "--graph", "study.sx"]).unwrap();
        assert!(matches!(command, CliCommand::Boot(_)));
    }
}
// conformance: standard study verb registration and bootloader handoff.
