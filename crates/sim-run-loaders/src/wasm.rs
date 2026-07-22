use std::sync::Arc;

use sim_kernel::{CapabilityName, Cx, Lib, LibLoader, LibSource, Result};

use crate::{bytes_from_source, path_from_source, url_from_source};

const WASM_MAGIC: &[u8; 4] = b"\0asm";
const WASM_LOAD_CAPABILITY: &str = "loader.wasm";

/// Capability required to instantiate wasm component libs.
pub fn wasm_load_capability() -> CapabilityName {
    CapabilityName::new(WASM_LOAD_CAPABILITY)
}

/// Loader for `.wasm` component libs, backed by a host wasm runtime.
///
/// A wasm `site` export is registered as an opaque value under its placement
/// symbol. Calling that value invokes the guest's `<placement>/realize`
/// operation through the bounded wasm host call path; agent libraries adapt the
/// value into an `EvalSite`.
pub struct WasmLoader {
    runtime: Arc<dyn sim_wasm_abi::WasmRuntime>,
}

impl WasmLoader {
    /// Creates a wasm loader that instantiates components on `runtime`.
    pub fn new(runtime: Arc<dyn sim_wasm_abi::WasmRuntime>) -> Self {
        Self { runtime }
    }
}

impl LibLoader for WasmLoader {
    fn can_load(&self, source: &LibSource) -> bool {
        if let Ok(Some(path)) = path_from_source(source) {
            return path.extension().is_some_and(|ext| ext == "wasm");
        }
        if let Ok(Some(bytes)) = bytes_from_source(source) {
            return bytes.get(..4) == Some(WASM_MAGIC.as_slice());
        }
        false
    }

    fn load(&self, cx: &mut Cx, source: LibSource) -> Result<Box<dyn Lib>> {
        cx.require(&wasm_load_capability())?;

        if let Some(path) = path_from_source(&source)? {
            return Ok(Box::new(sim_wasm_abi::load_wasm_lib_from_file(
                self.runtime.clone(),
                path,
            )?));
        }
        if let Some(bytes) = bytes_from_source(&source)? {
            return Ok(Box::new(sim_wasm_abi::load_wasm_lib_from_bytes(
                self.runtime.clone(),
                &bytes,
            )?));
        }
        if let Some(url) = url_from_source(&source)? {
            return Err(sim_kernel::Error::HostError(format!(
                "url loading is not implemented for wasm source {url}"
            )));
        }
        Err(sim_kernel::Error::HostError(
            "wasm loader received unsupported source".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{AbiVersion, Args, Cx, DefaultFactory, Expr, LibLoader, LibTarget, Symbol};
    use sim_wasm_abi::{
        AbiValue, Frame, InMemoryWasmRuntime, WasmExport, WasmGuestModule, WasmManifest,
        decode_value_frame, encode_exports_frame, encode_manifest_frame, encode_value_frame,
    };

    use super::*;

    const WASM_SITE_BYTES: &[u8] = b"\0asmloader-wasm-site";

    struct SiteGuest;

    impl WasmGuestModule for SiteGuest {
        fn manifest_frame(&self) -> sim_kernel::Result<Frame> {
            encode_manifest_frame(&wasm_site_manifest())
        }

        fn exports_frame(&self) -> sim_kernel::Result<Frame> {
            encode_exports_frame(&wasm_site_manifest().exports)
        }

        fn call(&self, function: &Symbol, args: Frame) -> sim_kernel::Result<Frame> {
            assert_eq!(function.to_string(), "model/loader-wasm-site/realize");
            let AbiValue::Expr(Expr::List(args)) = decode_value_frame(&args)? else {
                return Err(sim_kernel::Error::TypeMismatch {
                    expected: "expr list args",
                    found: "non-list args",
                });
            };
            assert!(matches!(
                args.as_slice(),
                [Expr::String(text)] if text == "loader request"
            ));
            encode_value_frame(&AbiValue::Expr(Expr::String(
                "loader wasm answer".to_owned(),
            )))
        }
    }

    #[test]
    fn wasm_site_export_loads_from_bytes_and_registers_site_value() {
        let runtime = Arc::new(InMemoryWasmRuntime::new());
        runtime
            .register_module(WASM_SITE_BYTES, Arc::new(SiteGuest))
            .unwrap();
        let loader = WasmLoader::new(runtime);
        let source = crate::bytes_source(WASM_SITE_BYTES.to_vec());
        assert!(loader.can_load(&source));

        let mut cx = Cx::new(
            Arc::new(sim_kernel::NoopEvalPolicy),
            Arc::new(DefaultFactory),
        );
        cx.grant(wasm_load_capability());
        let lib = loader.load(&mut cx, source).unwrap();
        cx.load_lib(lib.as_ref()).unwrap();

        let site_symbol = wasm_site_symbol();
        let arg = cx.factory().string("loader request".to_owned()).unwrap();
        let site = cx.registry().site_by_symbol(&site_symbol).unwrap().clone();
        assert_eq!(
            site.object().as_expr(&mut cx).unwrap(),
            Expr::Symbol(site_symbol)
        );
        let reply = site
            .object()
            .as_callable()
            .unwrap()
            .call(&mut cx, Args::new(vec![arg]))
            .unwrap();
        assert_eq!(
            reply.object().as_expr(&mut cx).unwrap(),
            Expr::String("loader wasm answer".to_owned())
        );
    }

    #[test]
    fn wasm_loader_requires_wasm_load_capability() {
        let runtime = Arc::new(InMemoryWasmRuntime::new());
        let loader = WasmLoader::new(runtime);
        let source = crate::bytes_source(WASM_SITE_BYTES.to_vec());
        let mut cx = Cx::new(
            Arc::new(sim_kernel::NoopEvalPolicy),
            Arc::new(DefaultFactory),
        );

        let err = match loader.load(&mut cx, source) {
            Ok(_) => panic!("wasm loader should require loader.wasm"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            sim_kernel::Error::CapabilityDenied { capability }
                if capability.as_str() == WASM_LOAD_CAPABILITY
        ));
    }

    fn wasm_site_manifest() -> WasmManifest {
        WasmManifest {
            id: Symbol::qualified("test", "loader-wasm-site"),
            version: "0.1.0".to_owned(),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::WasmComponent,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![WasmExport::Site {
                symbol: wasm_site_symbol(),
            }],
        }
    }

    fn wasm_site_symbol() -> Symbol {
        Symbol::qualified("model", "loader-wasm-site")
    }
}
