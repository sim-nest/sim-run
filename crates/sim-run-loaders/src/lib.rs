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
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod shape;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
mod shared;
#[cfg(any(feature = "wasm", test))]
mod wasm;

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub use native::{
    NativeDylibLoader, NativeGuest, encode_native_manifest_response, validate_native_abi_header,
};
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub use native_macro::NativeAbiMacro;
#[cfg(any(feature = "wasm", test))]
pub use wasm::{WasmLoader, wasm_load_capability};
