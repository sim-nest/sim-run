use std::sync::Arc;

use sim_kernel::{
    Cx, Expr, Factory, MatchScore, Result, Shape, ShapeDoc, ShapeMatch, ShapeRef, Symbol, Value,
};

#[derive(Clone)]
pub(crate) struct NativeAnyShape {
    symbol: Symbol,
}

impl NativeAnyShape {
    pub(crate) fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }
}

impl Shape for NativeAnyShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }

    fn is_total(&self) -> bool {
        true
    }

    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(self.symbol.to_string()).with_detail("native ABI proxy shape"))
    }
}

pub(crate) fn shape_value(symbol: Symbol, shape: Arc<dyn Shape>) -> ShapeRef {
    sim_kernel::DefaultFactory
        .opaque(Arc::new(NamedNativeShape { symbol, shape }))
        .expect("native shape object should always be boxable")
}

struct NamedNativeShape {
    symbol: Symbol,
    shape: Arc<dyn Shape>,
}

impl Shape for NamedNativeShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }

    fn parents(&self, cx: &mut Cx) -> Result<Vec<ShapeRef>> {
        self.shape.parents(cx)
    }

    fn is_effectful(&self) -> bool {
        self.shape.is_effectful()
    }

    fn is_total(&self) -> bool {
        self.shape.is_total()
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        self.shape.is_subshape_of(cx, parent)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        self.shape.check_value(cx, value)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        self.shape.check_expr(cx, expr)
    }

    fn describe(&self, cx: &mut Cx) -> Result<ShapeDoc> {
        self.shape.describe(cx)
    }
}
