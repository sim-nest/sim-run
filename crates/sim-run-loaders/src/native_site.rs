use std::sync::Arc;

use sim_kernel::{
    Args, Callable, Cx, Expr, Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value,
};

use super::native::NativeGuest;

#[derive(Clone)]
struct NativeAbiSite {
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
}

impl NativeAbiSite {
    fn new(guest: Arc<dyn NativeGuest>, symbol: Symbol) -> Self {
        Self { guest, symbol }
    }
}

impl Object for NativeAbiSite {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-site {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for NativeAbiSite {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        let symbol = Symbol::qualified("core", "Expr");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(sim_kernel::CORE_EXPR_CLASS_ID, symbol)
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_table(&self, cx: &mut Cx) -> Result<sim_kernel::TableRef> {
        cx.factory().table(vec![(
            Symbol::new("symbol"),
            cx.factory().string(self.symbol.to_string())?,
        )])
    }
}

impl Callable for NativeAbiSite {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let expr_args = args
            .values()
            .iter()
            .map(|value| value.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()?;
        let arg_bytes = sim_codec_binary::encode_frame(&Expr::List(expr_args))?.0;
        let bytes = self
            .guest
            .invoke(&format!("{}/realize", self.symbol), &arg_bytes)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        cx.factory().expr(expr)
    }
}

pub(super) fn register_native_site(
    cx: &mut LoadCx,
    linker: &mut Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
) -> Result<()> {
    let site = cx
        .factory()
        .opaque(Arc::new(NativeAbiSite::new(guest, symbol.clone())))?;
    linker.site_value(symbol, site)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{
        AbiVersion, Cx, DefaultFactory, Export, Lib, LibManifest, LibTarget, NoopEvalPolicy,
        Version,
    };

    use super::*;

    struct StubNativeSiteLib {
        manifest: LibManifest,
        guest: Arc<dyn NativeGuest>,
    }

    impl Lib for StubNativeSiteLib {
        fn manifest(&self) -> LibManifest {
            self.manifest.clone()
        }

        fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
            for export in &self.manifest.exports {
                if let Export::Site { symbol, .. } = export {
                    register_native_site(cx, linker, self.guest.clone(), symbol.clone())?;
                }
            }
            Ok(())
        }
    }

    struct MockGuest;

    impl NativeGuest for MockGuest {
        fn invoke(&self, op: &str, args: &[u8]) -> Result<Vec<u8>> {
            assert_eq!(op, "model/local/realize");
            let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
            assert!(matches!(expr, Expr::List(_)));
            Ok(sim_codec_binary::encode_frame(&Expr::Symbol(Symbol::new("ok")))?.0)
        }
    }

    #[test]
    fn native_site_manifest_export_loads_and_registers_site_value() {
        let site_symbol = Symbol::qualified("model", "local");
        let lib = StubNativeSiteLib {
            manifest: LibManifest {
                id: Symbol::new("native-site-test"),
                version: Version("0.1.0".to_owned()),
                abi: AbiVersion { major: 0, minor: 1 },
                target: LibTarget::HostRegistered,
                requires: Vec::new(),
                capabilities: Vec::new(),
                exports: vec![Export::Site {
                    symbol: site_symbol.clone(),
                    runtime_id: None,
                }],
            },
            guest: Arc::new(MockGuest),
        };
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

        cx.load_lib(&lib).unwrap();

        let site = cx.registry().site_by_symbol(&site_symbol).unwrap().clone();
        assert_eq!(
            site.object().as_expr(&mut cx).unwrap(),
            Expr::Symbol(site_symbol)
        );
        let result = site
            .object()
            .as_callable()
            .unwrap()
            .call(&mut cx, Args::default())
            .unwrap();
        assert_eq!(
            result.object().as_expr(&mut cx).unwrap(),
            Expr::Symbol(Symbol::new("ok"))
        );
    }
}
