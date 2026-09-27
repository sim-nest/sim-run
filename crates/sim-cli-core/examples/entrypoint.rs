// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_cli_core::{CliEntrypoint, cli_main_entrypoint_symbol};
use sim_kernel::Symbol;

fn main() {
    for name in ["world", "index"] {
        let entry = CliEntrypoint {
            lib: Symbol::qualified("lib", name),
            symbol: cli_main_entrypoint_symbol(name),
        };
        assert_eq!(
            entry.symbol,
            Symbol::qualified("cli", format!("main/{name}"))
        );
        println!("{} -> {}", entry.lib, entry.symbol);
    }
}
