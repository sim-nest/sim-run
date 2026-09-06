use sim_run_core::{CliCommand, LoadSession};

const VERB: &str = "repl";
const HOST: &str = "lib/repl";

pub(crate) fn with_repl_if_selected(command: &CliCommand, session: LoadSession) -> LoadSession {
    if !matches!(command, CliCommand::Boot(boot) if boot.payload.eval.is_some()
        || boot.payload.args.first().is_some_and(|arg| arg == VERB || arg == "eval"))
    {
        return session;
    }
    session
        .with_host_factory(HOST, || Box::new(sim_lib_repl::ReplLib::new()))
        .with_capability(sim_kernel::macro_expand_capability())
        .with_capability(sim_kernel::macro_expand_eval_capability())
}
