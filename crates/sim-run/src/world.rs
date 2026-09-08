// conformance: the bootloader selects world only for the explicit world verb.

use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const VERB: &str = sim_lib_world::WORLD_VERB;
const HOST: &str = "lib/world-command";

pub(crate) fn with_world_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_world_command(command) {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(HOST, || {
            Box::new(
                sim_lib_world::WorldCommandLib::new()
                    .expect("the bundled world provider is a checked build-time invariant"),
            )
        })
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(HOST.to_owned()),
            ],
        )
}

fn is_world_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == VERB)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_run_core::parse_args;

    #[test]
    fn detects_only_world_payload() {
        assert!(is_world_command(
            &parse_args([
                "sim",
                "world",
                "why",
                "conclusion/source-api",
                "source/public-api"
            ])
            .unwrap()
        ));
        assert!(!is_world_command(
            &parse_args(["sim", "index", "list"]).unwrap()
        ));
    }
}
