use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = sim_lib_search::SEARCH_VERB;
const HOST: &str = "lib/search-command";

pub(crate) fn with_search_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_search_command(command) {
        return session;
    }
    session
        .with_host_factory(HOST, || Box::new(sim_lib_search::SearchCommandLib::new()))
        .with_default_verb_sources(VERB, vec![LibSourceSpec::Host(HOST.to_owned())])
}

fn is_search_command(command: &CliCommand) -> bool {
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
    fn detects_only_search_payload() {
        assert!(is_search_command(
            &parse_args(["sim", "search", "query", "sim"]).unwrap()
        ));
        assert!(!is_search_command(
            &parse_args(["sim", "index", "list"]).unwrap()
        ));
    }
}
