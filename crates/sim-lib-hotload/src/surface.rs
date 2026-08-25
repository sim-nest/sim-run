//! Loadable, data-only hot-generation operations.

use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{
    AbiVersion, Args, Callable, CapabilityName, ClassRef, Cx, Error, Export, Factory, Lib,
    LibManifest, LibTarget, Linker, LoadCx, MatchScore, Object, ObjectCompat, ObjectEncode,
    ObjectEncoding, Ref, Result, Shape, ShapeDoc, ShapeMatch, ShapeRef, Symbol, Value, Version,
    card::Card,
};
use sim_shape::shape_value;

/// The five stable operations exported by [`HotloadLib`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotloadOperation {
    /// Construct immutable candidate bytes in the configured sandbox.
    Build,
    /// Inspect and test a candidate outside the live context.
    Admit,
    /// Atomically install an admitted generation.
    Activate,
    /// Inspect the current generation and mutation state.
    Status,
    /// Inspect durable generation and refusal evidence.
    History,
}

impl HotloadOperation {
    const ALL: [Self; 5] = [
        Self::Build,
        Self::Admit,
        Self::Activate,
        Self::Status,
        Self::History,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Admit => "admit",
            Self::Activate => "activate",
            Self::Status => "status",
            Self::History => "history",
        }
    }

    fn capability(self) -> CapabilityName {
        hotload_capability(match self {
            Self::Build => "build",
            Self::Activate => "activate",
            Self::Admit => "admit",
            Self::Status | Self::History => "inspect",
        })
    }

    fn help(self) -> &'static str {
        match self {
            Self::Build => {
                "build a sealed package; returns candidate identity and sandbox evidence"
            }
            Self::Admit => {
                "check compatibility and bounded tests without touching the live context"
            }
            Self::Activate => "activate an admission receipt atomically and durably",
            Self::Status => {
                "inspect generation identity, reachability, sandbox controls, and audit state"
            }
            Self::History => {
                "browse completed generations, refusals, compatibility differences, and journal evidence"
            }
        }
    }
}

/// Data-only request or outcome record. Its constructor encoding produces
/// `#(hotload/Record KIND {FIELDS...})` in Lisp data position.
#[derive(Clone, Debug)]
pub struct HotloadRecord {
    kind: Symbol,
    fields: BTreeMap<Symbol, sim_kernel::Expr>,
}

impl HotloadRecord {
    /// Creates a canonical record. Field order is normalized by symbol.
    pub fn new(kind: Symbol, fields: impl IntoIterator<Item = (Symbol, sim_kernel::Expr)>) -> Self {
        Self {
            kind,
            fields: fields.into_iter().collect(),
        }
    }

    /// Record kind.
    pub fn kind(&self) -> &Symbol {
        &self.kind
    }

    /// Canonically ordered public fields.
    pub fn fields(&self) -> &BTreeMap<Symbol, sim_kernel::Expr> {
        &self.fields
    }

    fn from_value(cx: &mut Cx, value: &Value) -> Result<Self> {
        if let Some(record) = value.object().as_any().downcast_ref::<Self>() {
            return Ok(record.clone());
        }
        let expr = value.object().as_expr(cx)?;
        let sim_kernel::Expr::Map(entries) = expr else {
            return Err(Error::TypeMismatch {
                expected: "hotload record",
                found: "non-record",
            });
        };
        let mut kind = None;
        let mut fields = BTreeMap::new();
        for (key, value) in entries {
            let sim_kernel::Expr::Symbol(key) = key else {
                continue;
            };
            if key == Symbol::new("kind") {
                let sim_kernel::Expr::Symbol(value) = value else {
                    return Err(Error::TypeMismatch {
                        expected: "record kind symbol",
                        found: "non-symbol",
                    });
                };
                kind = Some(value);
            } else {
                fields.insert(key, value);
            }
        }
        Ok(Self::new(
            kind.ok_or_else(|| Error::Eval("hotload record has no kind".into()))?,
            fields,
        ))
    }
}

impl Object for HotloadRecord {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<hotload-record {}>", self.kind))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for HotloadRecord {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0x484f_544c_4f41_4401),
            record_class_symbol(),
        )
    }
    fn as_expr(&self, _cx: &mut Cx) -> Result<sim_kernel::Expr> {
        let mut entries = vec![(
            sim_kernel::Expr::Symbol(Symbol::new("kind")),
            sim_kernel::Expr::Symbol(self.kind.clone()),
        )];
        entries.extend(
            self.fields
                .iter()
                .map(|(key, value)| (sim_kernel::Expr::Symbol(key.clone()), value.clone())),
        );
        Ok(sim_kernel::Expr::Map(entries))
    }
    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for HotloadRecord {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: record_class_symbol(),
            args: vec![
                sim_kernel::Expr::Symbol(self.kind.clone()),
                sim_kernel::Expr::Map(
                    self.fields
                        .iter()
                        .map(|(k, v)| (sim_kernel::Expr::Symbol(k.clone()), v.clone()))
                        .collect(),
                ),
            ],
        })
    }
}

/// Host-provided orchestration membrane. Implementations compose the existing
/// builder, admission, activation, and journal services; values crossing this
/// boundary are records only.
pub trait HotloadPort: Send + Sync {
    /// Performs one typed operation.
    fn invoke(
        &self,
        operation: HotloadOperation,
        request: &HotloadRecord,
    ) -> std::result::Result<HotloadRecord, HotloadRecord>;
}

/// Loadable hot-generation library over an injected orchestration membrane.
pub struct HotloadLib {
    port: Arc<dyn HotloadPort>,
}

impl HotloadLib {
    /// Composes the loadable surface with an installed host orchestration port.
    pub fn new(port: Arc<dyn HotloadPort>) -> Self {
        Self { port }
    }
}

impl Lib for HotloadLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: hotload_lib_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: exports(),
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        let mut contracts = Vec::new();
        for operation in HotloadOperation::ALL {
            let args = operation_shape(operation, "Args", "one hotload/Record request", true);
            let result =
                operation_shape(operation, "Result", "typed hotload/Record outcome", false);
            linker.shape_value(shape_symbol(operation, "Args"), args.clone())?;
            linker.shape_value(shape_symbol(operation, "Result"), result.clone())?;
            linker.function_value(
                operation_symbol(operation),
                cx.factory().opaque(Arc::new(HotloadFunction {
                    operation,
                    port: Arc::clone(&self.port),
                    args: args.clone(),
                    result: result.clone(),
                }))?,
            )?;
            contracts.push((operation, args, result));
        }
        linker.value(
            hotload_operation_cards_symbol(),
            cx.factory().list(cards(cx.factory(), &contracts)?)?,
        )?;
        Ok(())
    }
}

struct HotloadFunction {
    operation: HotloadOperation,
    port: Arc<dyn HotloadPort>,
    args: Value,
    result: Value,
}
impl Object for HotloadFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", operation_symbol(self.operation)))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for HotloadFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for HotloadFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let values = args.into_vec();
        let [value] = values.as_slice() else {
            return Err(Error::Eval(format!(
                "{} expects one record",
                operation_symbol(self.operation)
            )));
        };
        cx.require(&self.operation.capability())?;
        let request = HotloadRecord::from_value(cx, value)?;
        let outcome = self
            .port
            .invoke(self.operation, &request)
            .unwrap_or_else(|refusal| refusal);
        cx.factory().opaque(Arc::new(outcome))
    }
    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.args.clone()))
    }
    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.result.clone()))
    }
}

struct RecordShape {
    symbol: Symbol,
    operation: HotloadOperation,
    args: bool,
    detail: &'static str,
}
impl Shape for RecordShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        if self.args {
            let expr = value.object().as_expr(cx)?;
            let sim_kernel::Expr::List(items) = expr else {
                return Ok(ShapeMatch::reject("arguments must be a list"));
            };
            if items.len() != 1 {
                return Ok(ShapeMatch::reject("exactly one record is required"));
            }
        } else if HotloadRecord::from_value(cx, &value).is_err() {
            return Ok(ShapeMatch::reject("result must be a hotload record"));
        }
        Ok(ShapeMatch::accept(MatchScore::exact(100)))
    }
    fn check_expr(&self, _cx: &mut Cx, expr: &sim_kernel::Expr) -> Result<ShapeMatch> {
        let accepted = if self.args {
            matches!(expr, sim_kernel::Expr::List(items) if items.len() == 1)
        } else {
            matches!(expr, sim_kernel::Expr::Map(_))
        };
        Ok(if accepted {
            ShapeMatch::accept(MatchScore::exact(100))
        } else {
            ShapeMatch::reject("hotload record shape mismatch")
        })
    }
    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!(
            "{} {}",
            operation_symbol(self.operation),
            if self.args { "arguments" } else { "result" }
        ))
        .with_detail(self.detail))
    }
}

fn operation_shape(
    operation: HotloadOperation,
    suffix: &str,
    detail: &'static str,
    args: bool,
) -> Value {
    let symbol = shape_symbol(operation, suffix);
    shape_value(
        symbol.clone(),
        Arc::new(RecordShape {
            symbol,
            operation,
            args,
            detail,
        }),
    )
}
fn cards(
    factory: &dyn Factory,
    contracts: &[(HotloadOperation, Value, Value)],
) -> Result<Vec<Value>> {
    contracts
        .iter()
        .map(|(operation, args, result)| {
            let symbol = operation_symbol(*operation);
            let entries = vec![
                (Symbol::new("subject"), factory.symbol(symbol.clone())?),
                (
                    Symbol::new("kind"),
                    factory.symbol(Symbol::qualified("hotload", "operation"))?,
                ),
                (
                    Symbol::new("help"),
                    factory.string(operation.help().to_owned())?,
                ),
                (Symbol::new("args"), args.clone()),
                (Symbol::new("result"), result.clone()),
                (Symbol::new("tests"), factory.list(Vec::new())?),
                (
                    Symbol::new("ops"),
                    factory.list(vec![factory.symbol(symbol.clone())?])?,
                ),
                (
                    Symbol::new("requires"),
                    factory.list(vec![factory.symbol(Symbol::qualified(
                        "capability",
                        operation.capability().as_str(),
                    ))?])?,
                ),
                (Symbol::new("see-also"), factory.list(Vec::new())?),
                (Symbol::new("shape-known"), factory.bool(true)?),
            ];
            factory.opaque(Arc::new(Card::new(Ref::Symbol(symbol), entries)))
        })
        .collect()
}

fn exports() -> Vec<Export> {
    let mut exports = vec![Export::Value {
        symbol: hotload_operation_cards_symbol(),
    }];
    for operation in HotloadOperation::ALL {
        exports.push(Export::Function {
            symbol: operation_symbol(operation),
            function_id: None,
        });
        exports.push(Export::Shape {
            symbol: shape_symbol(operation, "Args"),
            shape_id: None,
        });
        exports.push(Export::Shape {
            symbol: shape_symbol(operation, "Result"),
            shape_id: None,
        });
    }
    exports
}
fn operation_symbol(operation: HotloadOperation) -> Symbol {
    Symbol::qualified("hotload", operation.name())
}
fn shape_symbol(operation: HotloadOperation, suffix: &str) -> Symbol {
    Symbol::qualified(format!("hotload/{}", operation.name()), suffix)
}
fn record_class_symbol() -> Symbol {
    Symbol::qualified("hotload", "Record")
}
/// Stable library identity.
pub fn hotload_lib_symbol() -> Symbol {
    Symbol::qualified("lib", "hotload")
}
/// Stable Cards export.
pub fn hotload_operation_cards_symbol() -> Symbol {
    Symbol::qualified("hotload", "operation-cards")
}
/// Stable operation symbols in product order.
pub fn hotload_operation_symbols() -> Vec<Symbol> {
    HotloadOperation::ALL
        .into_iter()
        .map(operation_symbol)
        .collect()
}
/// Returns one narrowly scoped hotload capability.
pub fn hotload_capability(name: &str) -> CapabilityName {
    CapabilityName::new(format!("hotload/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecordingPort(Mutex<Vec<HotloadOperation>>);
    impl HotloadPort for RecordingPort {
        fn invoke(
            &self,
            operation: HotloadOperation,
            request: &HotloadRecord,
        ) -> std::result::Result<HotloadRecord, HotloadRecord> {
            self.0.lock().unwrap().push(operation);
            Ok(HotloadRecord::new(
                Symbol::qualified("hotload", "ok"),
                [(
                    Symbol::new("request-kind"),
                    sim_kernel::Expr::Symbol(request.kind().clone()),
                )],
            ))
        }
    }

    #[test]
    fn manifest_exports_five_functions_shapes_and_cards() {
        let lib = HotloadLib::new(Arc::new(RecordingPort(Mutex::new(Vec::new()))));
        let manifest = lib.manifest();
        for symbol in hotload_operation_symbols() {
            assert!(manifest.exports.iter().any(|export| matches!(export, Export::Function { symbol: found, .. } if found == &symbol)));
        }
        assert_eq!(manifest.capabilities, Vec::new());
    }

    #[test]
    fn records_encode_without_host_authority() {
        let record = HotloadRecord::new(
            Symbol::qualified("hotload", "candidate"),
            [(
                Symbol::new("generation"),
                sim_kernel::Expr::String("sha256:abc".into()),
            )],
        );
        let mut cx = Cx::new(
            Arc::new(sim_kernel::NoopEvalPolicy),
            Arc::new(sim_kernel::DefaultFactory),
            sim_kernel::HandleSeed::new(9),
        );
        let ObjectEncoding::Constructor { class, args } = record.object_encoding(&mut cx).unwrap()
        else {
            panic!("constructor encoding")
        };
        assert_eq!(class, record_class_symbol());
        let text = format!("{args:?}");
        for forbidden in ["path", "argv", "handle", "loader", "provider"] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn callable_enforces_capability_and_returns_typed_idempotent_outcome() {
        let port = Arc::new(RecordingPort(Mutex::new(Vec::new())));
        let (mut cx, seat) = Cx::new_seated(
            Arc::new(sim_kernel::NoopEvalPolicy),
            Arc::new(sim_kernel::DefaultFactory),
            sim_kernel::HandleSeed::new(10),
        );
        cx.load_lib(&HotloadLib::new(port.clone())).unwrap();
        let function = cx
            .registry()
            .function_by_symbol(&operation_symbol(HotloadOperation::Build))
            .unwrap()
            .clone();
        let request = cx
            .factory()
            .opaque(Arc::new(HotloadRecord::new(
                Symbol::qualified("hotload", "build-request"),
                Vec::new(),
            )))
            .unwrap();
        assert!(function
            .object()
            .as_callable()
            .unwrap()
            .call(&mut cx, Args::new(vec![request.clone()]))
            .is_err());
        seat.grant(&mut cx, hotload_capability("build")).unwrap();
        let first = function
            .object()
            .as_callable()
            .unwrap()
            .call(&mut cx, Args::new(vec![request.clone()]))
            .unwrap();
        let second = function
            .object()
            .as_callable()
            .unwrap()
            .call(&mut cx, Args::new(vec![request]))
            .unwrap();
        assert!(first.object().as_any().is::<HotloadRecord>());
        assert!(second.object().as_any().is::<HotloadRecord>());
        assert_eq!(port.0.lock().unwrap().as_slice(), [HotloadOperation::Build, HotloadOperation::Build]);
    }
}
