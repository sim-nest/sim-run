use sim_run_core::{CliCommand, LoadSession};

pub(crate) fn with_expr_tree_if_selected(
    command: &CliCommand,
    session: LoadSession,
) -> LoadSession {
    if !is_expr_tree_command(command) {
        return session;
    }
    sim_lib_expr_tree_serve::configure_expr_tree_session(session)
}

fn is_expr_tree_command(command: &CliCommand) -> bool {
    let CliCommand::Boot(boot) = command else {
        return false;
    };
    boot.payload
        .args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|verb| verb == sim_lib_expr_tree_serve::EXPR_TREE_VERB)
}

#[cfg(test)]
mod tests {
    use sim_run_core::parse_args;

    use super::is_expr_tree_command;

    #[test]
    fn detects_expression_tree_payload_verb() {
        let command = parse_args(["sim", "expr-tree"]).unwrap();
        assert!(is_expr_tree_command(&command));
    }

    #[test]
    fn non_expression_tree_payload_stays_on_default_boot_path() {
        let command = parse_args(["sim", "run"]).unwrap();
        assert!(!is_expr_tree_command(&command));
    }
}
