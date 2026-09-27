// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: canonical command symbols and owned descriptors need no runtime session.

use sim_cli_core::{CLI_MAIN_ENTRYPOINT, CliEntrypoint, cli_main_entrypoint_symbol};
use sim_kernel::Symbol;

#[test]
fn generic_marker_and_named_entrypoints_keep_their_exact_symbols() {
    assert_eq!(CLI_MAIN_ENTRYPOINT, "cli/main");
    for name in ["world", "index", "", "nested/name", " spaced ", "\u{e5}"] {
        let symbol = cli_main_entrypoint_symbol(name);
        assert_eq!(symbol, Symbol::qualified("cli", format!("main/{name}")));
        assert_eq!(symbol.to_string(), format!("cli/main/{name}"));
    }
    assert_ne!(
        cli_main_entrypoint_symbol(""),
        Symbol::new(CLI_MAIN_ENTRYPOINT)
    );
}

#[test]
fn descriptors_own_both_names_without_a_runtime_instance() {
    let entry = CliEntrypoint {
        lib: Symbol::qualified("lib", "world"),
        symbol: cli_main_entrypoint_symbol("world"),
    };
    let cloned = entry.clone();
    assert_eq!(cloned, entry);
    let other = CliEntrypoint {
        lib: Symbol::qualified("lib", "another-world"),
        symbol: entry.symbol.clone(),
    };
    assert_ne!(entry, other);
    assert_eq!(entry.symbol, other.symbol);
}
