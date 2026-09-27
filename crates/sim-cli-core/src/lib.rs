// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Command-entry names shared by loaded libraries and their dispatcher.
//!
//! This crate constructs inert symbols and entrypoint descriptors. It does not
//! load a library, select a command, create a context, or execute host effects.
//! The bootloader owns selection and dispatch; a command library owns behavior.
//!
//! ```
//! use sim_cli_core::{CLI_MAIN_ENTRYPOINT, CliEntrypoint, cli_main_entrypoint_symbol};
//! use sim_kernel::Symbol;
//!
//! let entry = CliEntrypoint {
//!     lib: Symbol::qualified("lib", "world"),
//!     symbol: cli_main_entrypoint_symbol("world"),
//! };
//! assert_eq!(CLI_MAIN_ENTRYPOINT, "cli/main");
//! assert_eq!(entry.symbol, Symbol::qualified("cli", "main/world"));
//! ```

mod entrypoint;

pub use entrypoint::{CLI_MAIN_ENTRYPOINT, CliEntrypoint, cli_main_entrypoint_symbol};
