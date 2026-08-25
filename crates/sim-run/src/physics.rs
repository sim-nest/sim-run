use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "physics";

pub(crate) fn with_physics_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(sim_lib_physics_runtime::HOST_ID, || {
            Box::new(sim_lib_physics_runtime::PhysicsRuntimeLib)
        })
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(sim_lib_physics_runtime::HOST_ID.into()),
            ],
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physics_selects_the_standard_bootloader_path() {
        let command = sim_run_core::parse_args(["sim", "physics", "browse"]).unwrap();
        assert!(matches!(command, CliCommand::Boot(_)));
    }
}
