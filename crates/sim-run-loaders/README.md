# sim-run-loaders

Low-level loader plugins for the `sim` bootloader.

This crate keeps source-kind payload helpers, Lisp source loading, binary lib
packs, native dynamic-library loading, and wasm loading below the SDK umbrella so
command-line feature composition can add loader mechanisms without importing the
full runtime facade.

Native and wasm guests may export `site` records. The loader registers each
site as an opaque runtime value under its placement symbol; the kernel stores
the value but does not implement `EvalSite`. Agent libraries adapt the value
through the model placement catalog.
