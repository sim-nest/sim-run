//! Standard-distribution registration for the storage-owned relation command.
// conformance: relation verb handoff and declared standard site.

use std::sync::Arc;

use sim_lib_relation_cli::{CommandError, RelationCommand, RelationCommandLib, RelationCommands};
use sim_run_core::{CliCommand, LibSourceSpec, LoadSession};

const VERB: &str = "relation";
const HOST: &str = "lib/relation-cli";

pub(crate) fn with_relation_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !is_relation_command(command) {
        return session;
    }
    session
        .with_host_factory(HOST, || {
            Box::new(RelationCommandLib::new(Arc::new(StandardDistribution)))
        })
        .with_default_verb_sources(VERB, vec![LibSourceSpec::Host(HOST.to_owned())])
}

fn is_relation_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == VERB)
}

/// Standard-distribution catalog. Concrete provider sessions are installed by
/// platform composition; the bootloader does not acquire storage itself.
struct StandardDistribution;
impl RelationCommands for StandardDistribution {
    fn execute(&self, command: &RelationCommand) -> Result<String, CommandError> {
        match command {
            RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::List,
                ..
            } => Ok("provider\tsite\nSQLite\trelation/site/sqlite\n".to_owned()),
            RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::Show,
                id: Some(id),
            } if id == "relation/site/sqlite" => Ok(
                "logical=relation/site/sqlite\nprovider=relation/provider/sqlite\nlocator=preopened|memory\n"
                    .to_owned(),
            ),
            _ => Err(CommandError::unavailable(
                "relation operation requires a declared site supplied by platform composition",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use sim_run_core::parse_args;

    use super::*;

    #[test]
    fn detects_only_relation_payload() {
        assert!(is_relation_command(
            &parse_args(["sim", "relation", "site", "list"]).unwrap()
        ));
        assert!(!is_relation_command(
            &parse_args(["sim", "index", "list"]).unwrap()
        ));
    }

    #[test]
    fn standard_distribution_declares_sqlite_site() {
        let output = StandardDistribution
            .execute(&RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::List,
                id: None,
            })
            .unwrap();
        assert!(output.contains("relation/site/sqlite"));
    }
}
