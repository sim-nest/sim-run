// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

fn site_list() -> String {
    let mut text = String::from("provider\tsite");
    text.push('\n');
    text.push_str("SQLite\t");
    text.push_str("relation/site/sqlite");
    text.push('\n');
    text
}

fn site_show() -> String {
    let mut text = String::from("logical=");
    text.push_str("relation/site/sqlite");
    text.push('\n');
    text.push_str("provider=");
    text.push_str("relation/provider/sqlite");
    text.push('\n');
    text.push_str("locator=preopened|memory");
    text.push('\n');
    text
}
impl RelationCommands for StandardDistribution {
    fn execute(&self, command: &RelationCommand) -> Result<String, CommandError> {
        match command {
            RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::List,
                ..
            } => Ok(site_list()),
            RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::Show,
                id: Some(id),
            } if id == "relation/site/sqlite" => Ok(site_show()),
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
        let mut listed = String::from("provider\tsite");
        listed.push('\n');
        listed.push_str("SQLite\t");
        listed.push_str("relation/site/sqlite");
        listed.push('\n');
        assert_eq!(output, listed);
        let show = StandardDistribution
            .execute(&RelationCommand::Site {
                action: sim_lib_relation_cli::ReadAction::Show,
                id: Some("relation/site/sqlite".to_owned()),
            })
            .unwrap();
        let mut shown = String::from("logical=");
        shown.push_str("relation/site/sqlite");
        shown.push('\n');
        shown.push_str("provider=");
        shown.push_str("relation/provider/sqlite");
        shown.push('\n');
        shown.push_str("locator=preopened|memory");
        shown.push('\n');
        assert_eq!(show, shown);
    }
}
