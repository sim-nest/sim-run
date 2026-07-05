use std::sync::Arc;

use sim_kernel::{
    Cx, Expr, Factory, MatchScore, Object, ObjectCompat, Result, Shape, ShapeDoc, ShapeMatch,
    Symbol,
};

use super::native::NativeGuest;

/// Host-side proxy for a macro exported by a native ABI guest.
#[derive(Clone)]
pub struct NativeAbiMacro {
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
}

impl NativeAbiMacro {
    /// Creates a macro proxy for `symbol`.
    pub fn new(guest: Arc<dyn NativeGuest>, symbol: Symbol) -> Self {
        Self { guest, symbol }
    }

    /// Returns the macro symbol.
    pub fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    /// Returns whether parser output from this macro is trusted.
    pub fn parser_trusted(&self) -> bool {
        true
    }

    /// Returns the syntax shape used by SDK macro expansion.
    pub fn syntax_shape(&self) -> Arc<dyn Shape> {
        Arc::new(NativeMacroSyntaxShape {
            symbol: self.symbol.clone(),
        })
    }

    /// Expands `input` by dispatching to `<symbol>/expand` in the guest.
    pub fn expand(&self, input: Expr) -> Result<Expr> {
        let args = sim_codec_binary::encode_frame(&input)?.0;
        let bytes = self
            .guest
            .invoke(&format!("{}/expand", self.symbol), &args)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        Ok(expr)
    }
}

impl Object for NativeAbiMacro {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-macro {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for NativeAbiMacro {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        let symbol = Symbol::qualified("core", "Macro");
        if let Some(value) = cx.registry().class_by_symbol(&symbol) {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(sim_kernel::CORE_MACRO_CLASS_ID, symbol)
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }

    fn as_table(&self, cx: &mut Cx) -> Result<sim_kernel::TableRef> {
        cx.factory().table(vec![
            (
                Symbol::new("symbol"),
                cx.factory().string(self.symbol.to_string())?,
            ),
            (
                Symbol::new("parser-trusted"),
                cx.factory().bool(self.parser_trusted())?,
            ),
        ])
    }
}

struct NativeMacroSyntaxShape {
    symbol: Symbol,
}

impl Shape for NativeMacroSyntaxShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified(
            self.symbol.to_string(),
            "native-macro-syntax",
        ))
    }

    fn check_value(&self, cx: &mut Cx, value: sim_kernel::Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        match expr {
            Expr::List(items) if matches!(items.first(), Some(Expr::Symbol(symbol)) if symbol == &self.symbol) => {
                Ok(ShapeMatch::accept(MatchScore::exact(1)))
            }
            _ => Ok(ShapeMatch::reject(format!(
                "expected list headed by {}",
                self.symbol
            ))),
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!("{} syntax", self.symbol)))
    }
}

pub(super) fn register_native_macro(
    linker: &mut sim_kernel::Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
) -> Result<()> {
    let mac = NativeAbiMacro::new(guest, symbol.clone());
    let value = sim_kernel::DefaultFactory
        .opaque(Arc::new(mac))
        .expect("native macro object should always be boxable");
    linker.macro_value(symbol, value)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{Expr, Symbol};

    use super::{NativeAbiMacro, NativeGuest};

    struct MockGuest;

    impl NativeGuest for MockGuest {
        fn invoke(&self, op: &str, args: &[u8]) -> sim_kernel::Result<Vec<u8>> {
            assert_eq!(op, "native/quote/expand");
            let (_, input) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
            let Expr::List(items) = input else {
                panic!("macro input should be a list");
            };
            Ok(sim_codec_binary::encode_frame(&items[1])?.0)
        }
    }

    #[test]
    fn native_macro_marshals_input_form_to_guest() {
        let mac = NativeAbiMacro::new(Arc::new(MockGuest), Symbol::qualified("native", "quote"));
        let expanded = mac
            .expand(Expr::List(vec![
                Expr::Symbol(Symbol::qualified("native", "quote")),
                Expr::String("value".to_owned()),
            ]))
            .unwrap();
        assert_eq!(expanded, Expr::String("value".to_owned()));
    }
}
