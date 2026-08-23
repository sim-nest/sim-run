use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "provider";
const HOST: &str = "lib/provider-cli";

pub(crate) fn with_provider_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_provider_command(command) {
        return session;
    }
    session
        .with_host_factory(HOST, || {
            Box::new(sim_lib_provider_cli::ProviderCommandLib::new())
        })
        .with_default_verb_sources(VERB, vec![LibSourceSpec::Host(HOST.to_owned())])
}

fn is_provider_command(command: &CliCommand) -> bool {
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
    fn detects_provider_payload_verb() {
        assert!(is_provider_command(
            &parse_args(["sim", "provider", "seats"]).unwrap()
        ));
        assert!(!is_provider_command(
            &parse_args(["sim", "index", "list"]).unwrap()
        ));
    }
}
