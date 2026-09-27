// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::Symbol;

/// Symbol prefix a loaded library claims for command-line execution.
pub const CLI_MAIN_ENTRYPOINT: &str = "cli/main";

/// Names a library and its exported command-entry function.
///
/// This owned data is not evidence that the library is loaded or callable and
/// conveys no execution authority. Selection and invocation belong to the
/// dispatcher; this descriptor retains no runtime instance or resource lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliEntrypoint {
    /// Library that exports the entrypoint.
    pub lib: Symbol,
    /// Exported function symbol used for the handoff.
    pub symbol: Symbol,
}

/// Builds the qualified `cli/main/NAME` entrypoint symbol for a named library.
///
/// The name is preserved verbatim, including an empty name or embedded slashes.
/// Construction performs no lookup, validation, normalization, or dispatch.
pub fn cli_main_entrypoint_symbol(name: &str) -> Symbol {
    Symbol::qualified("cli", format!("main/{name}"))
}
