use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const VERB: &str = "roadmap";
const HOST: &str = "lib/roadmap";

pub(crate) fn with_roadmap_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_roadmap_command(command) {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || Box::new(sim_lib_roadmap::RoadmapLib::new()))
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(HOST.into()),
            ],
        )
}

fn is_roadmap_command(command: &CliCommand) -> bool {
    matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_run_core::parse_args;

    #[test]
    fn registers_only_for_the_roadmap_verb() {
        assert!(is_roadmap_command(
            &parse_args(["sim", "roadmap", "plan"]).unwrap()
        ));
        for verb in ["run", "index", "provider", "road-map"] {
            assert!(
                !is_roadmap_command(&parse_args(["sim", verb]).unwrap()),
                "{verb}"
            );
        }
    }
}
