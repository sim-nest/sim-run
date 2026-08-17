use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

use crate::boot_codec::{BOOT_CODEC_HOST, BootCodec};

const INDEX_VERB: &str = "index";
const INDEX_APP_HOST: &str = "lib/index";

pub(crate) fn with_index_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_index_command(command) {
        return session;
    }
    session
        .with_host_factory(BOOT_CODEC_HOST, || Box::new(BootCodec))
        .with_host_factory(INDEX_APP_HOST, || Box::new(sim_lib_index::IndexLib::new()))
        .with_default_verb_sources(
            INDEX_VERB,
            vec![
                LibSourceSpec::Host(BOOT_CODEC_HOST.to_owned()),
                LibSourceSpec::Host(INDEX_APP_HOST.to_owned()),
            ],
        )
}

fn is_index_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == INDEX_VERB)
}

#[cfg(test)]
mod tests {
    use sim_run_core::parse_args;

    use super::is_index_command;

    #[test]
    fn detects_index_payload_verb() {
        let command = parse_args(["sim", "index", "list"]).unwrap();
        assert!(is_index_command(&command));
    }

    #[test]
    fn non_index_payload_stays_on_default_boot_path() {
        let command = parse_args(["sim", "run"]).unwrap();
        assert!(!is_index_command(&command));
    }
}
