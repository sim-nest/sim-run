# sim-cli-core

`sim-cli-core` owns the command-entry naming contract used by loaded libraries
and the bootloader. `CLI_MAIN_ENTRYPOINT` names the generic `cli/main` marker;
`cli_main_entrypoint_symbol(name)` constructs the exact `cli/main/NAME` symbol.
`CliEntrypoint` pairs that export symbol with its library symbol.

These are inert, owned values. They do not attest that a library is loaded,
retain a runtime instance, or authorize execution. The crate depends on the
kernel's symbol contract, not on loading, networking, or audio implementations.
Command selection, envelope conversion, execution, and cleanup belong to their
existing runtime owners.

Run `cargo run -p sim-cli-core --example entrypoint` for the checked naming
specimen. The `entrypoint` integration tests cover descriptor equality and
verbatim naming, including empty, nested, and non-ASCII input.
