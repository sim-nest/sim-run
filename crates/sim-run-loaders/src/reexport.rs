#[cfg(all(feature = "codec-lisp", feature = "shape"))]
use std::sync::Arc;

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
use sim_kernel::{Cx, Expr, Factory, Object, ObjectCompat, Shape, Symbol, Value};
#[cfg(any(feature = "codec-binary", feature = "codec-lisp"))]
use sim_kernel::{Lib, Result};
#[cfg(all(feature = "codec-lisp", feature = "shape"))]
use sim_shape::{AnyShape, CaptureShape, ListShape};

/// Kind of registry item a lib-pack or source-lib re-export links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReexportKind {
    /// Class re-export.
    Class,
    /// Function re-export.
    Function,
    /// Macro re-export.
    Macro,
    /// Shape re-export.
    Shape,
    /// Codec re-export.
    Codec,
    /// Number-domain re-export.
    NumberDomain,
    /// Runtime value re-export.
    Value,
}

/// One re-export entry mapping an exported symbol to a target already present
/// in the registry, tagged by the kind of item it links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReexportSpec {
    pub(crate) kind: ReexportKind,
    pub(crate) export: sim_kernel::Symbol,
    pub(crate) target: sim_kernel::Symbol,
}

impl ReexportSpec {
    /// Creates a re-export specification from its kind, exported symbol, and
    /// target symbol.
    pub fn new(kind: ReexportKind, export: sim_kernel::Symbol, target: sim_kernel::Symbol) -> Self {
        Self {
            kind,
            export,
            target,
        }
    }

    /// Returns the kind of registry item this spec links.
    pub fn kind(&self) -> ReexportKind {
        self.kind
    }

    /// Returns the symbol exposed by the loaded lib.
    pub fn export(&self) -> &sim_kernel::Symbol {
        &self.export
    }

    /// Returns the registry symbol linked to the exported symbol.
    pub fn target(&self) -> &sim_kernel::Symbol {
        &self.target
    }
}

#[cfg(any(
    feature = "codec-binary",
    all(feature = "codec-lisp", not(feature = "shape"))
))]
pub(crate) struct ReexportLib {
    manifest: sim_kernel::LibManifest,
    exports: Vec<ReexportSpec>,
}

#[cfg(any(
    feature = "codec-binary",
    all(feature = "codec-lisp", not(feature = "shape"))
))]
impl ReexportLib {
    pub(crate) fn new(manifest: sim_kernel::LibManifest, exports: Vec<ReexportSpec>) -> Self {
        Self { manifest, exports }
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceMacroSpec {
    pub(crate) symbol: sim_kernel::Symbol,
    pub(crate) fixed_params: Vec<sim_kernel::Symbol>,
    pub(crate) rest_param: Option<sim_kernel::Symbol>,
    pub(crate) template: sim_kernel::Expr,
}

/// Macro object emitted by a Lisp source lib for a source-authored `defmacro`.
#[cfg(all(feature = "codec-lisp", feature = "shape"))]
#[derive(Clone)]
pub struct SourceTemplateMacro {
    symbol: Symbol,
    syntax_shape: Arc<dyn Shape>,
    template: Expr,
    parser_trusted: bool,
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
impl SourceTemplateMacro {
    /// Creates a source template macro whose parser output is treated as
    /// untrusted.
    pub fn new(symbol: Symbol, syntax_shape: Arc<dyn Shape>, template: Expr) -> Self {
        Self {
            symbol,
            syntax_shape,
            template,
            parser_trusted: false,
        }
    }

    /// Returns the macro symbol.
    pub fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    /// Returns the syntax shape used to match macro calls.
    pub fn syntax_shape(&self) -> Arc<dyn Shape> {
        self.syntax_shape.clone()
    }

    /// Returns whether the parse output is trusted for effectful syntax
    /// shapes.
    pub fn parser_trusted(&self) -> bool {
        self.parser_trusted
    }

    /// Expands a matched macro form using the captured template bindings.
    pub fn expand(&self, _input: Expr, captures: sim_shape::Bindings) -> Result<Expr> {
        crate::lisp_source::template::instantiate_macro_template(&self.template, &captures)
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
impl Object for SourceTemplateMacro {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<source-macro {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
impl ObjectCompat for SourceTemplateMacro {
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
        let shape = self.syntax_shape();
        let doc = shape.describe(cx)?;
        let mut entries = vec![
            (
                Symbol::new("symbol"),
                cx.factory().string(self.symbol.to_string())?,
            ),
            (Symbol::new("syntax-shape"), cx.factory().string(doc.name)?),
            (
                Symbol::new("parser-trusted"),
                cx.factory().bool(self.parser_trusted)?,
            ),
        ];
        for (index, detail) in doc.details.into_iter().enumerate() {
            entries.push((
                Symbol::qualified("syntax-detail", index.to_string()),
                cx.factory().string(detail)?,
            ));
        }
        cx.factory().table(entries)
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
pub(crate) struct SourceLib {
    manifest: sim_kernel::LibManifest,
    exports: Vec<ReexportSpec>,
    macros: Vec<SourceMacroSpec>,
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
impl SourceLib {
    pub(crate) fn new(
        manifest: sim_kernel::LibManifest,
        exports: Vec<ReexportSpec>,
        macros: Vec<SourceMacroSpec>,
    ) -> Self {
        Self {
            manifest,
            exports,
            macros,
        }
    }
}

#[cfg(any(feature = "codec-binary", feature = "codec-lisp"))]
fn link_reexport(linker: &mut sim_kernel::Linker<'_>, export: &ReexportSpec) -> Result<()> {
    match export.kind {
        ReexportKind::Class => {
            let value = linker
                .registry()
                .class_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownClass {
                    class: export.target.clone(),
                })?;
            linker.class_value(export.export.clone(), value)?;
        }
        ReexportKind::Function => {
            let value = linker
                .registry()
                .function_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownFunction {
                    function: export.target.clone(),
                })?;
            linker.function_value(export.export.clone(), value)?;
        }
        ReexportKind::Macro => {
            let value = linker
                .registry()
                .macro_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownSymbol {
                    symbol: export.target.clone(),
                })?;
            linker.macro_value(export.export.clone(), value)?;
        }
        ReexportKind::Shape => {
            let value = linker
                .registry()
                .shape_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownSymbol {
                    symbol: export.target.clone(),
                })?;
            linker.shape_value(export.export.clone(), value)?;
        }
        ReexportKind::Codec => {
            let value = linker
                .registry()
                .codec_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownSymbol {
                    symbol: export.target.clone(),
                })?;
            linker.codec_value(export.export.clone(), value)?;
        }
        ReexportKind::NumberDomain => {
            let value = linker
                .registry()
                .number_domain_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownSymbol {
                    symbol: export.target.clone(),
                })?;
            linker.number_domain_value(export.export.clone(), value)?;
        }
        ReexportKind::Value => {
            let value = linker
                .registry()
                .value_by_symbol(&export.target)
                .cloned()
                .ok_or(sim_kernel::Error::UnknownSymbol {
                    symbol: export.target.clone(),
                })?;
            linker.value(export.export.clone(), value)?;
        }
    }
    Ok(())
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
impl Lib for SourceLib {
    fn manifest(&self) -> sim_kernel::LibManifest {
        self.manifest.clone()
    }

    fn load(
        &self,
        _cx: &mut sim_kernel::LoadCx,
        linker: &mut sim_kernel::Linker<'_>,
    ) -> Result<()> {
        for export in &self.exports {
            if matches!(export.kind, ReexportKind::Macro)
                && export.export == export.target
                && self.macros.iter().any(|mac| mac.symbol == export.export)
            {
                continue;
            }
            link_reexport(linker, export)?;
        }

        for mac in &self.macros {
            let syntax_shape = positional_macro_shape(
                mac.symbol.clone(),
                &mac.fixed_params,
                mac.rest_param.as_ref(),
            );
            let value = macro_value(Arc::new(SourceTemplateMacro::new(
                mac.symbol.clone(),
                syntax_shape,
                mac.template.clone(),
            )));
            linker.macro_value(mac.symbol.clone(), value)?;
        }

        Ok(())
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
fn macro_value(mac: Arc<SourceTemplateMacro>) -> Value {
    sim_kernel::DefaultFactory
        .opaque(mac)
        .expect("source macro object should always be boxable")
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
fn positional_macro_shape(head: Symbol, fixed: &[Symbol], rest: Option<&Symbol>) -> Arc<dyn Shape> {
    let fixed_tail = fixed
        .iter()
        .cloned()
        .map(|name| Arc::new(CaptureShape::new(name, Arc::new(AnyShape))) as Arc<dyn Shape>)
        .collect::<Vec<_>>();
    let items = std::iter::once(literal_head_shape(head))
        .chain(fixed_tail)
        .collect::<Vec<_>>();
    match rest {
        Some(rest) => Arc::new(ListShape::with_rest(
            items,
            Arc::new(CaptureShape::new(rest.clone(), Arc::new(AnyShape))),
        )),
        None => Arc::new(ListShape::new(items)),
    }
}

#[cfg(all(feature = "codec-lisp", feature = "shape"))]
fn literal_head_shape(symbol: Symbol) -> Arc<dyn Shape> {
    Arc::new(sim_shape::ExactExprShape::new(Expr::Symbol(symbol)))
}

#[cfg(any(
    feature = "codec-binary",
    all(feature = "codec-lisp", not(feature = "shape"))
))]
impl Lib for ReexportLib {
    fn manifest(&self) -> sim_kernel::LibManifest {
        self.manifest.clone()
    }

    fn load(
        &self,
        _cx: &mut sim_kernel::LoadCx,
        linker: &mut sim_kernel::Linker<'_>,
    ) -> Result<()> {
        for export in &self.exports {
            link_reexport(linker, export)?;
        }
        Ok(())
    }
}
