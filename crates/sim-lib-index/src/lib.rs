//! Loadable runtime exploration surface for the SIM Index.
//!
//! The crate exports `cli/main/index` for the `sim` bootloader. It decodes the
//! embedded public SIM Index snapshot through `codec/index`, exposes that graph
//! as an immutable [`IndexDir`], and renders list, show, find, route, trace, and
//! examples queries in stable text or JSON.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod args;
mod dir;
mod envelope;
mod error;
mod query;
mod render;
mod route;
mod snapshot;
mod verb;

pub use args::{Collection, IndexCommand, OutputMode, parse_index_args};
pub use dir::IndexDir;
pub use error::IndexError;
pub use query::{Hit, Query, Trace, examples, find, trace};
pub use render::render_command;
pub use route::{RouteMatch, RouteStepMatch, route};
pub use snapshot::{embedded_index_source, load_embedded_index_doc};
pub use verb::{IndexLib, index_entrypoint_symbol};
