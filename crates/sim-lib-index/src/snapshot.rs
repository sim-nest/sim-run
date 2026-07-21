//! Embedded SIM Index snapshot loading.

use sim_codec_index::{IndexCodec, IndexForm};
use sim_index_core::IndexDoc;

use crate::IndexError;

const INDEX_SOURCE: &str = include_str!("snapshot/index.sx");

/// Returns the embedded canonical `codec/index` source.
pub fn embedded_index_source() -> &'static str {
    INDEX_SOURCE
}

/// Decodes the embedded SIM Index snapshot into the shared graph model.
pub fn load_embedded_index_doc() -> Result<IndexDoc, IndexError> {
    IndexCodec
        .decode(IndexForm::Sx, INDEX_SOURCE)
        .map_err(|err| IndexError::new(format!("decode embedded SIM Index snapshot: {err}")))
}
