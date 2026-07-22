#![deny(unsafe_code)]
#![deny(missing_docs)]
//! Low-level loader plugins for the SIM bootloader.
//!
//! The crate owns loader mechanisms that the `sim` binary composes behind
//! feature gates. It deliberately depends on kernel and codec contracts rather
//! than the SDK umbrella.
//!
//! Native and wasm loaders both surface `site` exports as opaque registry
//! values keyed by placement symbols. The kernel stores the value and export
//! record; server and agent libraries give the value `EvalSite` behavior.

#[cfg(feature = "codec-binary")]
mod binary_pack;
#[cfg(any(feature = "codec-binary", feature = "codec-lisp"))]
mod expr;
#[cfg(feature = "codec-lisp")]
mod lisp_source;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod manifest;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_class;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_macro;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_number;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_site;
mod reexport;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod shape;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod shared;
mod source_kind;
#[cfg(any(feature = "wasm", test))]
mod wasm;

#[cfg(feature = "codec-binary")]
pub use binary_pack::{
    BinaryLibPack, BinaryPackLoader, decode_binary_lib_pack, encode_binary_lib_pack,
};
#[cfg(feature = "codec-lisp")]
pub use lisp_source::LispSourceLoader;
#[cfg(all(feature = "codec-lisp", feature = "codec-binary"))]
pub use lisp_source::{
    compile_lisp_source_pack, compile_lisp_source_text_to_pack,
    encode_lisp_source_text_to_binary_pack, export_lisp_source_file_to_binary_pack,
};
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub use native::{
    NativeDylibLoader, NativeGuest, encode_native_manifest_response, validate_native_abi_header,
};
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub use native_macro::NativeAbiMacro;
#[cfg(all(feature = "codec-lisp", feature = "shape"))]
pub use reexport::SourceTemplateMacro;
pub use reexport::{ReexportKind, ReexportSpec};
pub use source_kind::{
    BYTES_SOURCE_KIND, CONTENT_ADDRESS_SOURCE_KIND, PATH_SOURCE_KIND, URL_SOURCE_KIND,
    bytes_from_payload, bytes_from_source, bytes_source, bytes_source_kind, bytes_source_spec,
    catalog_bytes_source, catalog_content_address_source, catalog_path_source, catalog_url_source,
    content_address_payload, content_address_source, content_address_source_kind,
    content_address_source_spec, is_bytes_source, is_path_source, is_url_source, path_from_payload,
    path_from_source, path_payload, path_source, path_source_kind, path_source_spec,
    url_from_payload, url_from_source, url_source, url_source_kind, url_source_spec,
};
#[cfg(any(feature = "wasm", test))]
pub use wasm::{WasmLoader, wasm_load_capability};
