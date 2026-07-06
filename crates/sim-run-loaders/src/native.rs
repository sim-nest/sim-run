use std::sync::Arc;
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
use std::{
    ffi::{CStr, CString, OsStr, c_void},
    path::Path,
};

use sim_kernel::{Cx, Lib, LibLoader, LibSource, Result, Symbol};

/// Encodes a lib manifest as the native ABI manifest-call response.
pub fn encode_native_manifest_response(
    manifest: &sim_kernel::LibManifest,
) -> Result<sim_kernel::NativeAbiCallResponse> {
    let bytes = sim_codec_binary::encode_frame(&crate::manifest::manifest_to_expr(manifest))?.0;
    Ok(sim_kernel::NativeAbiCallResponse::success(
        sim_kernel::native_abi_owned_bytes(bytes),
    ))
}

/// Loader for native dynamic libraries that expose the stable native lib ABI.
///
/// A native `site` export is registered as an opaque value under its placement
/// symbol. Calling that value invokes the guest's `<placement>/realize`
/// operation; agent libraries adapt the value into an `EvalSite`.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub struct NativeDylibLoader;

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
struct LoadedNativeLib {
    shared: Arc<NativeAbiShared>,
    manifest: sim_kernel::LibManifest,
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl LoadedNativeLib {
    fn new(shared: Arc<NativeAbiShared>, manifest: sim_kernel::LibManifest) -> Self {
        Self { shared, manifest }
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl Lib for LoadedNativeLib {
    fn manifest(&self) -> sim_kernel::LibManifest {
        self.manifest.clone()
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut sim_kernel::Linker<'_>) -> Result<()> {
        let guest: Arc<dyn NativeGuest> = self.shared.clone();
        register_native_manifest_exports(cx, linker, guest, &self.manifest)
    }

    fn unload(&self, cx: &mut Cx, linker: &mut sim_kernel::Linker<'_>) -> Result<()> {
        let _ = (cx, linker);
        Ok(())
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub(super) struct NativeAbiShared {
    _library: libloading::Library,
    abi: sim_kernel::NativeLibAbiV1,
    instance: usize,
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl NativeAbiShared {
    fn new(
        library: libloading::Library,
        abi: sim_kernel::NativeLibAbiV1,
        instance: *mut c_void,
    ) -> Self {
        Self {
            _library: library,
            abi,
            instance: instance as usize,
        }
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
#[allow(unsafe_code)]
impl Drop for NativeAbiShared {
    fn drop(&mut self) {
        unsafe {
            (self.abi.destroy_instance)(self.instance as *mut c_void);
        }
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl LibLoader for NativeDylibLoader {
    fn can_load(&self, source: &LibSource) -> bool {
        matches!(source, LibSource::Path(path) if is_native_library(path))
    }

    #[allow(unsafe_code)]
    fn load(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        cx.require(&sim_kernel::native_dynamic_load_capability())?;

        let path = match source {
            LibSource::Path(path) => path,
            _ => {
                return Err(sim_kernel::Error::HostError(
                    "native dylib loader received unsupported source".to_owned(),
                ));
            }
        };

        let library = unsafe { libloading::Library::new(&path) }.map_err(|err| {
            sim_kernel::Error::HostError(format!(
                "failed to open native dylib {}: {err}",
                path.display()
            ))
        })?;

        let entrypoint = unsafe {
            library.get::<unsafe extern "C" fn() -> *const sim_kernel::NativeLibAbiV1>(
                sim_kernel::NATIVE_DYLIB_ENTRYPOINT_V1.as_bytes(),
            )
        }
        .map_err(|err| {
            sim_kernel::Error::HostError(format!(
                "failed to resolve {} from {}: {err}",
                sim_kernel::NATIVE_DYLIB_ENTRYPOINT_V1,
                path.display()
            ))
        })?;

        let abi_ptr = unsafe { entrypoint() };
        if abi_ptr.is_null() {
            return Err(sim_kernel::Error::HostError(format!(
                "native dylib {} returned a null ABI pointer",
                path.display()
            )));
        }

        // Probe only the version header first: the entrypoint guarantees at
        // least `HEADER_SIZE` bytes, so reading the header is in-bounds.
        let header =
            unsafe { std::ptr::read_unaligned(abi_ptr.cast::<sim_kernel::NativeLibAbiHeaderV1>()) };
        validate_native_abi_header(&header, &path)?;

        // F1: the header only proves `HEADER_SIZE` bytes; refuse to read the
        // full vtable (six function pointers) until `struct_size` proves the
        // pointee is at least a whole `NativeLibAbiV1`. Reading it otherwise is
        // an out-of-bounds `read_unaligned`.
        if header.struct_size < std::mem::size_of::<sim_kernel::NativeLibAbiV1>() {
            return Err(sim_kernel::Error::HostError(format!(
                "native dylib {} reported ABI struct size {} smaller than host minimum {}",
                path.display(),
                header.struct_size,
                std::mem::size_of::<sim_kernel::NativeLibAbiV1>()
            )));
        }

        let abi = unsafe { std::ptr::read_unaligned(abi_ptr) };

        let instance = unsafe { (abi.instantiate)() };
        if instance.is_null() {
            return Err(sim_kernel::Error::HostError(format!(
                "native dylib {} returned a null lib instance",
                path.display()
            )));
        }

        // Own the instance immediately: from here every early return (`?`)
        // drops `shared` and destroys the guest instance exactly once, so
        // manifest-fetch, decode, and conversion failures cannot leak it.
        let shared = Arc::new(NativeAbiShared::new(library, abi, instance));
        let bytes = shared.manifest_bytes()?;
        let (_, manifest_expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        let manifest = crate::manifest::expr_to_manifest(manifest_expr)?;
        // F2: trust is assigned by the loader, not by guest text. A native
        // dylib manifest that self-labels a non-native target (for example
        // `host-registered`, which would escape the untrusted-source eval
        // denial) is rejected outright rather than silently accepted.
        let manifest = native_manifest(manifest)?;
        Ok(Box::new(LoadedNativeLib::new(shared, manifest)))
    }
}

/// Copies an ABI owned-byte buffer into an owned `Vec`, validating its shape
/// first. An empty `(null, 0, 0)` descriptor yields an empty vector; a null
/// pointer with a positive length, or a length past the reported capacity, is
/// rejected before any slice is constructed (both would be undefined behavior).
///
/// The caller retains ownership of `bytes` and must destroy it exactly once
/// through the guest's `destroy_bytes` entry after a successful copy.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
#[allow(unsafe_code)]
fn copy_owned_bytes(what: &str, bytes: sim_kernel::NativeAbiOwnedBytes) -> Result<Vec<u8>> {
    if bytes.len == 0 {
        return Ok(Vec::new());
    }
    if bytes.ptr.is_null() || bytes.len > bytes.cap {
        return Err(sim_kernel::Error::HostError(format!(
            "{what} returned an invalid native byte buffer (null: {}, len {}, cap {})",
            bytes.ptr.is_null(),
            bytes.len,
            bytes.cap
        )));
    }
    let slice = unsafe { std::slice::from_raw_parts(bytes.ptr.cast_const(), bytes.len) };
    Ok(slice.to_vec())
}

/// Normalizes a native dylib manifest: the loader FORCES the target to
/// [`LibTarget::Native`](sim_kernel::LibTarget::Native), so trust is assigned by
/// loader authority rather than derived from the guest manifest text. A `.so` can
/// therefore never mint a trusted `host-registered` label to escalate its own
/// trust, even though a standard lib compiled as a native dylib carries a
/// `host-registered` target in its own manifest.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn native_manifest(mut manifest: sim_kernel::LibManifest) -> Result<sim_kernel::LibManifest> {
    manifest.target = sim_kernel::LibTarget::Native;
    Ok(manifest)
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn is_native_library(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("so" | "dylib" | "dll")
    )
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
/// Validates the stable native ABI header returned by a candidate dynamic lib.
pub fn validate_native_abi_header(
    header: &sim_kernel::NativeLibAbiHeaderV1,
    path: &Path,
) -> Result<()> {
    if header.struct_size < sim_kernel::NativeLibAbiV1::HEADER_SIZE {
        return Err(sim_kernel::Error::HostError(format!(
            "native dylib {} reported ABI struct size {} smaller than host header {}",
            path.display(),
            header.struct_size,
            sim_kernel::NativeLibAbiV1::HEADER_SIZE
        )));
    }
    if header.abi_major != sim_kernel::NATIVE_LIB_ABI_V1_MAJOR {
        return Err(sim_kernel::Error::HostError(format!(
            "native dylib {} reported unsupported native ABI {}.{}",
            path.display(),
            header.abi_major,
            header.abi_minor
        )));
    }
    Ok(())
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl NativeAbiShared {
    /// Fetches and copies the guest's encoded manifest bytes, translating an ABI
    /// error into a host error and freeing the guest allocation exactly once.
    #[allow(unsafe_code)]
    fn manifest_bytes(&self) -> Result<Vec<u8>> {
        let response = unsafe { (self.abi.manifest)(self.instance as *mut c_void) };
        if !response.error.is_null() {
            let message = unsafe {
                let error = &*response.error;
                if error.message.is_null() {
                    "native ABI manifest call failed without an error message".to_owned()
                } else {
                    CStr::from_ptr(error.message).to_string_lossy().into_owned()
                }
            };
            unsafe {
                (self.abi.destroy_error)(response.error);
            }
            return Err(sim_kernel::Error::HostError(message));
        }
        let bytes = copy_owned_bytes("native ABI manifest", response.bytes)?;
        unsafe {
            (self.abi.destroy_bytes)(response.bytes);
        }
        Ok(bytes)
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
#[derive(Clone)]
struct NativeAbiFunction {
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl sim_kernel::Object for NativeAbiFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-function {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for NativeAbiFunction {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&sim_kernel::Symbol::qualified("core", "Function"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            sim_kernel::Symbol::qualified("core", "Function"),
        )
    }
    fn as_callable(&self) -> Option<&dyn sim_kernel::Callable> {
        Some(self)
    }
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl sim_kernel::Callable for NativeAbiFunction {
    fn call(&self, cx: &mut Cx, args: sim_kernel::Args) -> Result<sim_kernel::Value> {
        let expr_args = args
            .values()
            .iter()
            .map(|value| value.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()?;
        let arg_bytes = sim_codec_binary::encode_frame(&sim_kernel::Expr::List(expr_args))?.0;
        let bytes = self.guest.invoke(&self.symbol.to_string(), &arg_bytes)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        cx.factory().expr(expr)
    }
}

/// A minimal "call the guest" surface, so the function and codec proxies (and
/// their tests) do not depend on the raw FFI vtable. The real implementation
/// marshals over the native ABI; tests substitute a mock guest.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
pub trait NativeGuest: Send + Sync {
    /// Invoke guest operation `op` with a binary-frame `args` payload and return
    /// the binary-frame response payload.
    fn invoke(&self, op: &str, args: &[u8]) -> Result<Vec<u8>>;
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl NativeGuest for NativeAbiShared {
    #[allow(unsafe_code)]
    fn invoke(&self, op: &str, args: &[u8]) -> Result<Vec<u8>> {
        let symbol = CString::new(op).map_err(|_| {
            sim_kernel::Error::HostError(format!(
                "native ABI op {op} contains an interior NUL byte"
            ))
        })?;
        let response = unsafe {
            (self.abi.call)(
                self.instance as *mut c_void,
                symbol.as_ptr(),
                sim_kernel::NativeAbiBorrowedBytes::borrow(args),
            )
        };
        if !response.error.is_null() {
            let message = unsafe {
                let error = &*response.error;
                if error.message.is_null() {
                    "native ABI call failed without an error message".to_owned()
                } else {
                    CStr::from_ptr(error.message).to_string_lossy().into_owned()
                }
            };
            unsafe {
                (self.abi.destroy_error)(response.error);
            }
            return Err(sim_kernel::Error::HostError(message));
        }
        let bytes = copy_owned_bytes("native ABI call", response.bytes)?;
        unsafe {
            (self.abi.destroy_bytes)(response.bytes);
        }
        Ok(bytes)
    }
}

/// Host-side proxy decoder for a codec exported by a native guest lib. Marshals
/// the input to the guest's decode operation and decodes the returned `Expr`.
/// Text input only for now (the eval codecs are text); bytes input is rejected.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
struct NativeAbiCodecDecoder {
    guest: Arc<dyn NativeGuest>,
    op: String,
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl sim_codec::Decoder for NativeAbiCodecDecoder {
    fn decode(
        &self,
        _cx: &mut sim_codec::ReadCx<'_>,
        input: sim_codec::Input,
    ) -> Result<sim_kernel::Expr> {
        let text = match input {
            sim_codec::Input::Text(text) => text,
            sim_codec::Input::Bytes(_) => {
                return Err(sim_kernel::Error::HostError(
                    "native codec proxy decode: bytes input is not yet supported".to_owned(),
                ));
            }
        };
        let args = sim_codec_binary::encode_frame(&sim_kernel::Expr::String(text))?.0;
        let bytes = self.guest.invoke(&self.op, &args)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        Ok(expr)
    }
}

/// Host-side proxy encoder for a native guest codec. Marshals the `Expr` to the
/// guest's encode operation and maps the returned text to `Output::Text`.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
struct NativeAbiCodecEncoder {
    guest: Arc<dyn NativeGuest>,
    op: String,
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
impl sim_codec::Encoder for NativeAbiCodecEncoder {
    fn encode(
        &self,
        _cx: &mut sim_kernel::WriteCx<'_>,
        expr: &sim_kernel::Expr,
    ) -> Result<sim_codec::Output> {
        let args = sim_codec_binary::encode_frame(expr)?.0;
        let bytes = self.guest.invoke(&self.op, &args)?;
        let (_, out) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        match out {
            sim_kernel::Expr::String(text) => Ok(sim_codec::Output::Text(text)),
            other => Err(sim_kernel::Error::HostError(format!(
                "native codec proxy encode: expected text output, got {other:?}"
            ))),
        }
    }
}

/// Builds a [`sim_codec::CodecRuntime`] whose plain decoder/encoder proxy to a
/// native guest. The located/tree variants are left unset (the runtime falls back
/// to the plain forms). Shapes resolve from the registry exactly as an in-process
/// codec does.
#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn native_codec_runtime(
    linker: &sim_kernel::Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
    codec_id: sim_kernel::CodecId,
) -> Result<sim_codec::CodecRuntime> {
    use sim_kernel::Factory;
    let factory = sim_kernel::DefaultFactory;
    let expr_shape = linker
        .registry()
        .shape_by_symbol(&Symbol::qualified("core", "Expr"))
        .or_else(|| {
            linker
                .registry()
                .shape_by_symbol(&Symbol::qualified("core", "Any"))
        })
        .cloned()
        .unwrap_or(factory.nil()?);
    let options_shape = linker
        .registry()
        .shape_by_symbol(&Symbol::qualified("core", "EncodeOptions"))
        .or_else(|| {
            linker
                .registry()
                .shape_by_symbol(&Symbol::qualified("core", "Any"))
        })
        .cloned()
        .unwrap_or(factory.nil()?);
    Ok(sim_codec::CodecRuntime {
        id: codec_id,
        symbol: symbol.clone(),
        decoder: Some(Arc::new(NativeAbiCodecDecoder {
            guest: guest.clone(),
            op: format!("{symbol}/decode"),
        })),
        located_decoder: None,
        tree_decoder: None,
        encoder: Some(Arc::new(NativeAbiCodecEncoder {
            guest,
            op: format!("{symbol}/encode"),
        })),
        located_encoder: None,
        tree_encoder: None,
        expr_shape,
        options_shape,
        default_decode: sim_codec::CodecDefaultDecode::TermInEvalDatumOtherwise,
    })
}

#[cfg(all(feature = "dynamic-native", not(target_arch = "wasm32")))]
fn register_native_manifest_exports(
    cx: &mut sim_kernel::LoadCx,
    linker: &mut sim_kernel::Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    manifest: &sim_kernel::LibManifest,
) -> Result<()> {
    for export in &manifest.exports {
        match export {
            sim_kernel::Export::Function { symbol, .. } => {
                let callable = cx.factory().opaque(Arc::new(NativeAbiFunction {
                    guest: guest.clone(),
                    symbol: symbol.clone(),
                }))?;
                linker.function_value(symbol.clone(), callable)?;
            }
            sim_kernel::Export::Class { symbol, class_id } => {
                super::native_class::register_native_class(
                    cx,
                    linker,
                    guest.clone(),
                    symbol.clone(),
                    *class_id,
                )?;
            }
            sim_kernel::Export::Macro { symbol, .. } => {
                super::native_macro::register_native_macro(linker, guest.clone(), symbol.clone())?;
            }
            sim_kernel::Export::Shape { symbol, .. } => {
                linker.shape_value(
                    symbol.clone(),
                    crate::shape::shape_value(
                        symbol.clone(),
                        Arc::new(crate::shape::NativeAnyShape::new(symbol.clone())),
                    ),
                )?;
            }
            sim_kernel::Export::Codec { symbol, codec_id } => {
                let id = codec_id.unwrap_or_else(|| cx.fresh_codec_id());
                let runtime = native_codec_runtime(linker, guest.clone(), symbol.clone(), id)?;
                linker.codec_value(symbol.clone(), sim_codec::codec_value(runtime))?;
            }
            sim_kernel::Export::NumberDomain { symbol, .. } => {
                super::native_number::register_native_number_domain(
                    cx,
                    linker,
                    guest.clone(),
                    symbol.clone(),
                )?;
            }
            sim_kernel::Export::Site { symbol, .. } => {
                super::native_site::register_native_site(
                    cx,
                    linker,
                    guest.clone(),
                    symbol.clone(),
                )?;
            }
            other => {
                return Err(sim_kernel::Error::Lib(format!(
                    "native dylib export {} {} is not yet supported by the stable native ABI",
                    other.kind(),
                    other.symbol()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_abi_guard_tests {
    use super::{copy_owned_bytes, native_manifest};
    use sim_kernel::{
        AbiVersion, Error, LibManifest, LibTarget, NativeAbiOwnedBytes, Symbol, Version,
    };

    fn manifest_with_target(target: LibTarget) -> LibManifest {
        LibManifest {
            id: Symbol::new("demo"),
            version: Version("0.1.0".to_owned()),
            abi: AbiVersion { major: 1, minor: 0 },
            target,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: Vec::new(),
        }
    }

    #[test]
    fn empty_owned_bytes_do_not_build_null_slice() {
        assert_eq!(
            copy_owned_bytes("test", NativeAbiOwnedBytes::empty()).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn null_pointer_with_positive_length_is_rejected() {
        let bogus = NativeAbiOwnedBytes {
            ptr: std::ptr::null_mut(),
            len: 8,
            cap: 8,
        };
        let err = copy_owned_bytes("test", bogus).expect_err("null ptr with len must be rejected");
        assert!(matches!(err, Error::HostError(m) if m.contains("invalid native byte buffer")));
    }

    #[test]
    fn length_beyond_capacity_is_rejected() {
        // A dangling non-null pointer: never dereferenced because len > cap
        // fails the guard before any slice is constructed.
        let mut byte = 0u8;
        let bogus = NativeAbiOwnedBytes {
            ptr: &mut byte,
            len: 64,
            cap: 1,
        };
        let err = copy_owned_bytes("test", bogus).expect_err("len past cap must be rejected");
        assert!(matches!(err, Error::HostError(m) if m.contains("invalid native byte buffer")));
    }

    #[test]
    fn native_manifest_forces_native_target_over_guest_host_registered() {
        // A guest `.so` cannot keep a trusted `host-registered` label: the loader
        // rewrites the target to Native so trust comes from loader authority.
        let manifest = native_manifest(manifest_with_target(LibTarget::HostRegistered)).unwrap();
        assert_eq!(manifest.target, LibTarget::Native);
    }

    #[test]
    fn native_manifest_accepts_native_target() {
        let manifest = native_manifest(manifest_with_target(LibTarget::Native)).unwrap();
        assert_eq!(manifest.target, LibTarget::Native);
    }
}

#[cfg(all(test, feature = "dynamic-native", not(target_arch = "wasm32")))]
mod native_codec_proxy_tests {
    use std::sync::Arc;

    use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, ReadPolicy, Symbol, WriteCx};

    use super::{NativeAbiCodecDecoder, NativeAbiCodecEncoder, NativeGuest};

    // A stand-in guest decodes the marshaled frame and returns a canned response,
    // so the proxy's marshal/unmarshal bridge is exercised without a real dylib.
    struct MockGuest;

    impl NativeGuest for MockGuest {
        fn invoke(&self, op: &str, args: &[u8]) -> sim_kernel::Result<Vec<u8>> {
            let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
            if op.ends_with("/decode") {
                assert!(
                    matches!(expr, Expr::String(_)),
                    "decode op should receive the input text as Expr::String"
                );
                Ok(sim_codec_binary::encode_frame(&Expr::Symbol(Symbol::new("parsed")))?.0)
            } else {
                Ok(sim_codec_binary::encode_frame(&Expr::String("rendered".to_owned()))?.0)
            }
        }
    }

    fn test_cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }

    #[test]
    fn proxy_decoder_marshals_text_to_guest_and_returns_expr() {
        let decoder = NativeAbiCodecDecoder {
            guest: Arc::new(MockGuest),
            op: "codec/mock/decode".to_owned(),
        };
        let mut cx = test_cx();
        let mut rcx = sim_codec::ReadCx {
            cx: &mut cx,
            codec: sim_kernel::CodecId(0),
            read_policy: ReadPolicy::default(),
            limits: sim_codec::DecodeLimits::default(),
        };
        let expr = sim_codec::Decoder::decode(
            &decoder,
            &mut rcx,
            sim_codec::Input::Text("(hello)".to_owned()),
        )
        .unwrap();
        assert_eq!(expr, Expr::Symbol(Symbol::new("parsed")));
    }

    #[test]
    fn proxy_encoder_marshals_expr_to_guest_and_returns_text() {
        let encoder = NativeAbiCodecEncoder {
            guest: Arc::new(MockGuest),
            op: "codec/mock/encode".to_owned(),
        };
        let mut cx = test_cx();
        let mut wcx = WriteCx {
            cx: &mut cx,
            codec: sim_kernel::CodecId(0),
            options: sim_kernel::EncodeOptions::default(),
        };
        let out = sim_codec::Encoder::encode(&encoder, &mut wcx, &Expr::Symbol(Symbol::new("x")))
            .unwrap();
        assert_eq!(out, sim_codec::Output::Text("rendered".to_owned()));
    }
}
