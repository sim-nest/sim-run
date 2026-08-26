use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};
use sim_lib_physics_runtime::{HOST_ID as PHYSICS_HOST, PhysicsRuntimeLib};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "physics";

pub(crate) fn with_physics_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.args.first().is_some_and(|arg| arg == VERB))
    {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(PHYSICS_HOST, || Box::new(PhysicsRuntimeLib))
        .with_default_verb_sources(
            VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.into()),
                LibSourceSpec::Host(PHYSICS_HOST.into()),
            ],
        )
}
