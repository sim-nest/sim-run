//! Loadable command-line REPL library for SIM.
//!
//! The crate exports `cli/main/repl` as a kernel `Function` and exposes
//! [`eval_line`] for in-process tests of the read-eval-print core. The eval
//! stack stays outside this crate: the context must already contain the selected
//! codec, number domains, and runtime functions.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod entrypoint;
mod eval;

pub use entrypoint::{ReplLib, repl_entrypoint_symbol};
pub use eval::{eval_line, run_repl_lines};
