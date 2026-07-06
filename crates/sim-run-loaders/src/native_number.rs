use std::sync::Arc;

use sim_kernel::{
    Cx, Expr, Linker, LoadCx, NumberBinaryOp, NumberDomain, NumberLiteral, NumberReductionOp,
    NumberUnaryOp, Object, ObjectCompat, Result, Symbol, Value, ValueNumberBinaryOp,
    ValueNumberReductionOp, ValueNumberUnaryOp,
};

use super::native::NativeGuest;

#[derive(Clone)]
struct NativeAbiNumberDomain {
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
}

impl NativeAbiNumberDomain {
    fn invoke_expr(&self, op: &str, expr: &Expr) -> Result<Expr> {
        let args = sim_codec_binary::encode_frame(expr)?.0;
        let bytes = self.guest.invoke(&format!("{}/{op}", self.symbol), &args)?;
        let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), &bytes)?;
        Ok(expr)
    }

    /// Re-validates a guest-returned number literal before the host accepts it
    /// (F19). A native number domain is untrusted guest code, so its canonical
    /// text is not taken verbatim: the literal must be labeled with this domain
    /// and its canonical string must be a fixed point of the domain's own
    /// parser (re-parsing the canonical form yields the identical canonical
    /// form). A foreign-domain or non-canonical literal is rejected rather than
    /// injected into the runtime.
    fn accept_number(&self, number: NumberLiteral) -> Result<NumberLiteral> {
        if number.domain != self.symbol {
            return Err(sim_kernel::Error::HostError(format!(
                "native number-domain {} returned a literal in foreign domain {}",
                self.symbol, number.domain
            )));
        }
        match self.invoke_expr("parse-literal", &Expr::String(number.canonical.clone()))? {
            Expr::Number(reparsed)
                if reparsed.domain == self.symbol && reparsed.canonical == number.canonical =>
            {
                Ok(number)
            }
            _ => Err(sim_kernel::Error::HostError(format!(
                "native number-domain {} returned a non-canonical literal {:?}",
                self.symbol, number.canonical
            ))),
        }
    }

    /// Converts a guest op result expression into a value, re-validating any
    /// number literal through [`accept_number`](Self::accept_number).
    fn value_from_number_expr(&self, cx: &mut Cx, expr: Expr) -> Result<Value> {
        match expr {
            Expr::Number(number) => {
                let number = self.accept_number(number)?;
                cx.factory().number_literal(number.domain, number.canonical)
            }
            other => cx.factory().expr(other),
        }
    }
}

impl Object for NativeAbiNumberDomain {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<native-number-domain {}>", self.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for NativeAbiNumberDomain {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        if let Some(value) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "NumberDomain"))
        {
            return Ok(value.clone());
        }
        cx.factory().class_stub(
            sim_kernel::CORE_NUMBER_DOMAIN_CLASS_ID,
            Symbol::qualified("core", "NumberDomain"),
        )
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.symbol.clone()))
    }

    fn as_number_domain(&self) -> Option<&dyn NumberDomain> {
        Some(self)
    }
}

impl NumberDomain for NativeAbiNumberDomain {
    fn symbol(&self) -> Symbol {
        self.symbol.clone()
    }

    fn parse_literal(&self, cx: &mut Cx, text: &str) -> Result<Option<Value>> {
        match self.invoke_expr("parse-literal", &Expr::String(text.to_owned()))? {
            Expr::Nil => Ok(None),
            Expr::Number(number) => {
                let number = self.accept_number(number)?;
                cx.factory()
                    .number_literal(number.domain, number.canonical)
                    .map(Some)
            }
            other => Err(sim_kernel::Error::HostError(format!(
                "native number-domain {} parse-literal returned non-number {other:?}",
                self.symbol
            ))),
        }
    }

    fn encode_literal(&self, cx: &mut Cx, value: Value) -> Result<Option<NumberLiteral>> {
        let expr = value.object().as_expr(cx)?;
        match self.invoke_expr("encode-literal", &expr)? {
            Expr::Nil => Ok(None),
            Expr::Number(number) => Ok(Some(number)),
            other => Err(sim_kernel::Error::HostError(format!(
                "native number-domain {} encode-literal returned non-number {other:?}",
                self.symbol
            ))),
        }
    }
}

pub(super) fn register_native_number_domain(
    cx: &mut LoadCx,
    linker: &mut Linker<'_>,
    guest: Arc<dyn NativeGuest>,
    symbol: Symbol,
) -> Result<()> {
    let value = cx.factory().opaque(Arc::new(NativeAbiNumberDomain {
        guest,
        symbol: symbol.clone(),
    }))?;
    linker.number_domain_value(symbol.clone(), value)?;
    register_native_scalar_ops(linker, symbol);
    Ok(())
}

fn register_native_scalar_ops(linker: &mut Linker<'_>, domain: Symbol) {
    for (operator, literal_apply, value_apply) in [
        (
            math_symbol("add"),
            native_number_add as fn(&mut Cx, NumberLiteral, NumberLiteral) -> Result<Value>,
            native_value_number_add as fn(&mut Cx, Value, Value) -> Result<Value>,
        ),
        (
            math_symbol("sub"),
            native_number_sub,
            native_value_number_sub,
        ),
        (
            math_symbol("mul"),
            native_number_mul,
            native_value_number_mul,
        ),
        (
            math_symbol("div"),
            native_number_div,
            native_value_number_div,
        ),
    ] {
        linker.number_binary_op(NumberBinaryOp {
            operator: operator.clone(),
            left_domain: domain.clone(),
            right_domain: domain.clone(),
            cost: 0,
            apply: literal_apply,
        });
        linker.value_number_binary_op(ValueNumberBinaryOp {
            operator,
            left_domain: domain.clone(),
            right_domain: domain.clone(),
            cost: 1,
            apply: value_apply,
        });
    }

    linker.number_unary_op(NumberUnaryOp {
        operator: math_symbol("neg"),
        operand_domain: domain.clone(),
        cost: 0,
        apply: native_number_neg,
    });
    linker.value_number_unary_op(ValueNumberUnaryOp {
        operator: math_symbol("neg"),
        operand_domain: domain.clone(),
        cost: 1,
        apply: native_value_number_neg,
    });

    for (operator, literal_apply, value_apply) in [
        (
            math_symbol("sum"),
            native_number_sum as fn(&mut Cx, Vec<NumberLiteral>) -> Result<Value>,
            native_value_number_sum as fn(&mut Cx, Vec<Value>) -> Result<Value>,
        ),
        (
            math_symbol("product"),
            native_number_product,
            native_value_number_product,
        ),
    ] {
        linker.number_reduction_op(NumberReductionOp {
            operator: operator.clone(),
            operand_domain: domain.clone(),
            cost: 0,
            apply: literal_apply,
        });
        linker.value_number_reduction_op(ValueNumberReductionOp {
            operator,
            operand_domain: domain.clone(),
            cost: 1,
            apply: value_apply,
        });
    }
}

fn math_symbol(name: &str) -> Symbol {
    Symbol::qualified("math", name)
}

fn native_number_add(cx: &mut Cx, left: NumberLiteral, right: NumberLiteral) -> Result<Value> {
    native_binary_number_op(cx, "add", left, right)
}

fn native_number_sub(cx: &mut Cx, left: NumberLiteral, right: NumberLiteral) -> Result<Value> {
    native_binary_number_op(cx, "sub", left, right)
}

fn native_number_mul(cx: &mut Cx, left: NumberLiteral, right: NumberLiteral) -> Result<Value> {
    native_binary_number_op(cx, "mul", left, right)
}

fn native_number_div(cx: &mut Cx, left: NumberLiteral, right: NumberLiteral) -> Result<Value> {
    native_binary_number_op(cx, "div", left, right)
}

fn native_number_neg(cx: &mut Cx, operand: NumberLiteral) -> Result<Value> {
    native_unary_number_op(cx, "neg", operand)
}

fn native_number_sum(cx: &mut Cx, operands: Vec<NumberLiteral>) -> Result<Value> {
    native_reduction_number_op(cx, "sum", operands)
}

fn native_number_product(cx: &mut Cx, operands: Vec<NumberLiteral>) -> Result<Value> {
    native_reduction_number_op(cx, "product", operands)
}

fn native_value_number_add(cx: &mut Cx, left: Value, right: Value) -> Result<Value> {
    let left = expect_literal(cx, left, "left")?;
    let right = expect_literal(cx, right, "right")?;
    native_number_add(cx, left, right)
}

fn native_value_number_sub(cx: &mut Cx, left: Value, right: Value) -> Result<Value> {
    let left = expect_literal(cx, left, "left")?;
    let right = expect_literal(cx, right, "right")?;
    native_number_sub(cx, left, right)
}

fn native_value_number_mul(cx: &mut Cx, left: Value, right: Value) -> Result<Value> {
    let left = expect_literal(cx, left, "left")?;
    let right = expect_literal(cx, right, "right")?;
    native_number_mul(cx, left, right)
}

fn native_value_number_div(cx: &mut Cx, left: Value, right: Value) -> Result<Value> {
    let left = expect_literal(cx, left, "left")?;
    let right = expect_literal(cx, right, "right")?;
    native_number_div(cx, left, right)
}

fn native_value_number_neg(cx: &mut Cx, operand: Value) -> Result<Value> {
    let operand = expect_literal(cx, operand, "operand")?;
    native_number_neg(cx, operand)
}

fn native_value_number_sum(cx: &mut Cx, operands: Vec<Value>) -> Result<Value> {
    let operands = expect_literals(cx, operands, "operand")?;
    native_number_sum(cx, operands)
}

fn native_value_number_product(cx: &mut Cx, operands: Vec<Value>) -> Result<Value> {
    let operands = expect_literals(cx, operands, "operand")?;
    native_number_product(cx, operands)
}

fn native_binary_number_op(
    cx: &mut Cx,
    op: &str,
    left: NumberLiteral,
    right: NumberLiteral,
) -> Result<Value> {
    if left.domain != right.domain {
        return Err(sim_kernel::Error::Eval(format!(
            "native number op {op} requires matching domains, got {} and {}",
            left.domain, right.domain
        )));
    }
    invoke_native_number_op(cx, &left.domain.clone(), op, &[left, right])
}

fn native_unary_number_op(cx: &mut Cx, op: &str, operand: NumberLiteral) -> Result<Value> {
    invoke_native_number_op(cx, &operand.domain.clone(), op, &[operand])
}

fn native_reduction_number_op(
    cx: &mut Cx,
    op: &str,
    operands: Vec<NumberLiteral>,
) -> Result<Value> {
    let Some(first) = operands.first() else {
        return Err(sim_kernel::Error::Eval(format!(
            "native number op {op} requires at least one operand"
        )));
    };
    let domain = first.domain.clone();
    if operands.iter().any(|operand| operand.domain != domain) {
        return Err(sim_kernel::Error::Eval(format!(
            "native number op {op} requires matching operand domains"
        )));
    }
    invoke_native_number_op(cx, &domain, op, &operands)
}

fn invoke_native_number_op(
    cx: &mut Cx,
    domain: &Symbol,
    op: &str,
    operands: &[NumberLiteral],
) -> Result<Value> {
    let domain_value = cx
        .registry()
        .number_domain_by_symbol(domain)
        .cloned()
        .ok_or_else(|| sim_kernel::Error::UnknownSymbol {
            symbol: domain.clone(),
        })?;
    let Some(native_domain) = domain_value
        .object()
        .downcast_ref::<NativeAbiNumberDomain>()
    else {
        return Err(sim_kernel::Error::HostError(format!(
            "number domain {domain} is not a native ABI proxy"
        )));
    };
    let args = Expr::List(operands.iter().cloned().map(Expr::Number).collect());
    let result = native_domain.invoke_expr(op, &args)?;
    native_domain.value_from_number_expr(cx, result)
}

fn expect_literal(cx: &mut Cx, value: Value, side: &str) -> Result<NumberLiteral> {
    cx.number_value_ref(value)?
        .and_then(|number| number.literal)
        .ok_or_else(|| {
            sim_kernel::Error::Eval(format!(
                "native number op {side} operand has no literal representation"
            ))
        })
}

fn expect_literals(cx: &mut Cx, values: Vec<Value>, side: &str) -> Result<Vec<NumberLiteral>> {
    values
        .into_iter()
        .map(|value| expect_literal(cx, value, side))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::{DefaultFactory, NoopEvalPolicy};

    struct MockGuest;

    impl NativeGuest for MockGuest {
        fn invoke(&self, op: &str, args: &[u8]) -> Result<Vec<u8>> {
            let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
            let out = match op {
                "numbers/f64/parse-literal" => match expr {
                    Expr::String(text) if text == "1.5" => Expr::Number(NumberLiteral {
                        domain: Symbol::qualified("numbers", "f64"),
                        canonical: text,
                    }),
                    Expr::String(text) if text == "bad-shape" => Expr::String(text),
                    Expr::String(_) => Expr::Nil,
                    other => panic!("unexpected parse input {other:?}"),
                },
                "numbers/f64/encode-literal" => expr,
                other => panic!("unexpected op {other}"),
            };
            Ok(sim_codec_binary::encode_frame(&out)?.0)
        }
    }

    #[test]
    fn native_number_domain_proxy_marshals_parse_and_encode() {
        let domain = NativeAbiNumberDomain {
            guest: Arc::new(MockGuest),
            symbol: Symbol::qualified("numbers", "f64"),
        };
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

        let parsed = domain.parse_literal(&mut cx, "1.5").unwrap().unwrap();
        assert_eq!(
            parsed.object().as_expr(&mut cx).unwrap(),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "1.5".to_owned(),
            })
        );
        assert!(domain.parse_literal(&mut cx, "nope").unwrap().is_none());
        assert!(matches!(
            domain.parse_literal(&mut cx, "bad-shape").unwrap_err(),
            sim_kernel::Error::HostError(message)
                if message.contains("parse-literal returned non-number")
        ));

        let encoded = domain.encode_literal(&mut cx, parsed).unwrap().unwrap();
        assert_eq!(encoded.domain, Symbol::qualified("numbers", "f64"));
        assert_eq!(encoded.canonical, "1.5");
    }

    // F19: a guest number domain is untrusted; its canonical form is re-checked.
    struct F19Guest;

    impl NativeGuest for F19Guest {
        fn invoke(&self, op: &str, args: &[u8]) -> Result<Vec<u8>> {
            let (_, expr) = sim_codec_binary::decode_frame(sim_kernel::CodecId(0), args)?;
            let f64_domain = Symbol::qualified("numbers", "f64");
            let out = match op {
                "numbers/f64/parse-literal" => match expr {
                    // Canonical text that is NOT a fixed point of parse: re-parsing
                    // "NONCANON" yields a different canonical ("other").
                    Expr::String(t) if t == "noncanon" => Expr::Number(NumberLiteral {
                        domain: f64_domain,
                        canonical: "NONCANON".to_owned(),
                    }),
                    Expr::String(t) if t == "NONCANON" => Expr::Number(NumberLiteral {
                        domain: f64_domain,
                        canonical: "other".to_owned(),
                    }),
                    // A literal mislabeled with a foreign domain.
                    Expr::String(t) if t == "foreign" => Expr::Number(NumberLiteral {
                        domain: Symbol::qualified("numbers", "i64"),
                        canonical: "1".to_owned(),
                    }),
                    // A well-formed canonical literal (fixed point of parse).
                    Expr::String(t) if t == "3" => Expr::Number(NumberLiteral {
                        domain: f64_domain,
                        canonical: "3".to_owned(),
                    }),
                    _ => Expr::Nil,
                },
                other => panic!("unexpected op {other}"),
            };
            Ok(sim_codec_binary::encode_frame(&out)?.0)
        }
    }

    #[test]
    fn guest_noncanonical_or_foreign_number_is_rejected() {
        let domain = NativeAbiNumberDomain {
            guest: Arc::new(F19Guest),
            symbol: Symbol::qualified("numbers", "f64"),
        };
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

        let err = domain.parse_literal(&mut cx, "noncanon").unwrap_err();
        assert!(matches!(
            err,
            sim_kernel::Error::HostError(message) if message.contains("non-canonical")
        ));

        let err = domain.parse_literal(&mut cx, "foreign").unwrap_err();
        assert!(matches!(
            err,
            sim_kernel::Error::HostError(message) if message.contains("foreign domain")
        ));

        let accepted = domain.parse_literal(&mut cx, "3").unwrap().unwrap();
        assert_eq!(
            accepted.object().as_expr(&mut cx).unwrap(),
            Expr::Number(NumberLiteral {
                domain: Symbol::qualified("numbers", "f64"),
                canonical: "3".to_owned(),
            })
        );
    }
}
