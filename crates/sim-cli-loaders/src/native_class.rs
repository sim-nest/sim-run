use std::sync::Arc;

use sim_kernel::{
    Args, Callable, Class, ClassId, ClassRef, Cx, Object, ObjectCompat, ReadConstructorRef, Result,
    ShapeRef, Symbol, TableRef, Value,
};

use super::native::NativeGuest;

#[derive(Clone)]
struct NativeAbiClass {
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
    id: ClassId,
}

#[derive(Clone)]
struct NativeAbiClassInstance {
    class_symbol: Symbol,
    class_id: ClassId,
    expr: sim_kernel::Expr,
}

impl Object for NativeAbiClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-class {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for NativeAbiClass {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "Class"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_CLASS_CLASS_ID,
            Symbol::qualified("core", "Class"),
        )
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<sim_kernel::Expr> {
        Ok(sim_kernel::Expr::Symbol(self.symbol.clone()))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}

impl Callable for NativeAbiClass {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let expr_args = args
            .values()
            .iter()
            .map(|value| value.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()?;
        let arg_bytes = sim_codec_binary::encode_frame(&sim_kernel::Expr::List(expr_args))?.0;
        let bytes = self
            .guest
            .invoke(&format!("{}/new", self.symbol), &arg_bytes)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        let object = parse_object_expr(&expr)?;
        if object.class != self.symbol {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "native class constructor result",
                found: "different class",
            });
        }
        cx.factory().opaque(Arc::new(NativeAbiClassInstance {
            class_symbol: self.symbol.clone(),
            class_id: self.id,
            expr,
        }))
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(None)
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(any_shape_value(Symbol::qualified(
            self.symbol.to_string(),
            "instance-shape",
        ))?))
    }
}

impl Class for NativeAbiClass {
    fn id(&self) -> ClassId {
        self.id
    }

    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ClassRef>> {
        Ok(Vec::new())
    }

    fn constructor_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        any_shape_value(Symbol::qualified(
            self.symbol.to_string(),
            "constructor-shape",
        ))
    }

    fn instance_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        any_shape_value(Symbol::qualified(self.symbol.to_string(), "instance-shape"))
    }

    fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<ReadConstructorRef>> {
        Ok(None)
    }

    fn members(&self, cx: &mut Cx) -> Result<TableRef> {
        cx.factory().table(Vec::new())
    }
}

impl Object for NativeAbiClassInstance {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-instance {}>", self.class_symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for NativeAbiClassInstance {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(value) = cx.registry().class_by_symbol(&self.class_symbol) {
            return Ok(value.clone());
        }
        cx.factory()
            .class_stub(self.class_id, self.class_symbol.clone())
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<sim_kernel::Expr> {
        Ok(self.expr.clone())
    }

    fn as_table(&self, cx: &mut Cx) -> Result<TableRef> {
        let Ok(object) = parse_object_expr(&self.expr) else {
            return cx.factory().table(Vec::new());
        };
        cx.factory().table(
            object
                .fields
                .into_iter()
                .map(|(symbol, expr)| Ok((symbol, cx.factory().expr(expr)?)))
                .collect::<Result<Vec<_>>>()?,
        )
    }
}

fn any_shape_value(symbol: Symbol) -> Result<ShapeRef> {
    Ok(crate::shape::shape_value(
        symbol.clone(),
        Arc::new(crate::shape::NativeAnyShape::new(symbol)),
    ))
}

struct NativeObjectExpr {
    class: Symbol,
    fields: Vec<(Symbol, sim_kernel::Expr)>,
}

fn parse_object_expr(expr: &sim_kernel::Expr) -> Result<NativeObjectExpr> {
    let sim_kernel::Expr::Extension { tag, payload } = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "object",
            found: "non-object",
        });
    };
    if *tag != Symbol::qualified("expr", "object") {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "expr/object",
            found: "different extension",
        });
    }
    let sim_kernel::Expr::Map(entries) = payload.as_ref() else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "object map",
            found: "non-map",
        });
    };
    let class = map_field(entries, &Symbol::new("class")).and_then(|expr| match expr {
        sim_kernel::Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "class symbol",
            found: "non-symbol",
        }),
    })?;
    let fields = map_field(entries, &Symbol::new("fields")).and_then(|expr| match expr {
        sim_kernel::Expr::Map(fields) => fields
            .iter()
            .map(|(key, value)| match key {
                sim_kernel::Expr::Symbol(symbol) => Ok((symbol.clone(), value.clone())),
                _ => Err(sim_kernel::Error::TypeMismatch {
                    expected: "field symbol",
                    found: "non-symbol",
                }),
            })
            .collect::<Result<Vec<_>>>(),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "fields map",
            found: "non-map",
        }),
    })?;
    Ok(NativeObjectExpr { class, fields })
}

fn map_field<'a>(
    entries: &'a [(sim_kernel::Expr, sim_kernel::Expr)],
    field: &Symbol,
) -> Result<&'a sim_kernel::Expr> {
    entries
        .iter()
        .find_map(|(key, value)| match key {
            sim_kernel::Expr::Symbol(symbol) if symbol == field => Some(value),
            _ => None,
        })
        .ok_or_else(|| sim_kernel::Error::UnknownSymbol {
            symbol: field.clone(),
        })
}

pub(super) fn register_native_class(
    cx: &mut sim_kernel::LoadCx,
    linker: &mut sim_kernel::Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
    class_id: Option<ClassId>,
) -> Result<()> {
    let id = class_id.unwrap_or_else(|| cx.fresh_class_id());
    let class = cx.factory().opaque(Arc::new(NativeAbiClass {
        guest,
        symbol: symbol.clone(),
        id,
    }))?;
    linker.class_with_id(symbol, id)?;
    linker.bind_class_value(id, class)
}
