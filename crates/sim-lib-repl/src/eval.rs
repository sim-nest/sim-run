use std::io::{BufRead, Write};
use std::sync::Arc;

use sim_codec::{
    DecodePosition, DecodedForm, Input, Output, decode_default_with_codec, encode_with_codec,
};
use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, EncodeOptions, Expr, ReadPolicy, Shape, Symbol, TrustLevel,
    macro_expand_eval_capability, read_eval_capability,
};
use sim_lib_core::{
    ReadEvalBroker, ReadEvalRequest, ReadEvalSource, RequestOrigin, install_read_eval_broker,
    read_eval_broker_symbol,
};
use sim_shape::AnyShape;

/// Options for a user-requested REPL/CLI eval admission.
#[derive(Clone)]
pub struct ReplEvalOptions {
    /// Capabilities the caller must already hold before eval can run.
    pub requires: Vec<CapabilityName>,
    /// Maximum powers the eval body may run with.
    pub allow: CapabilitySet,
    /// Shape the evaluated result must satisfy before printing.
    pub expected_shape: Arc<dyn Shape>,
}

impl Default for ReplEvalOptions {
    fn default() -> Self {
        Self {
            requires: Vec::new(),
            allow: CapabilitySet::new().grant(macro_expand_eval_capability()),
            expected_shape: Arc::new(AnyShape),
        }
    }
}

/// Decodes, evaluates, and re-encodes one source line through `codec`.
#[cfg(test)]
fn eval_line_for_tests(cx: &mut Cx, codec: &Symbol, line: &str) -> Result<String, String> {
    let decoded = decode_eval_expr(cx, codec, line)?;
    let value = cx.eval_expr(decoded).map_err(|err| format!("{err:?}"))?;
    let expr = value
        .object()
        .as_expr(cx)
        .map_err(|err| format!("{err:?}"))?;
    match encode_with_codec(cx, codec, &expr, EncodeOptions::default())
        .map_err(|err| format!("{err:?}"))?
    {
        Output::Text(text) => Ok(text),
        Output::Bytes(_) => Ok("<bytes>".to_owned()),
    }
}

/// Admits explicitly requested eval through the shared read-eval broker.
pub fn eval_requested_text(cx: &mut Cx, codec: &Symbol, source: &str) -> Result<String, String> {
    eval_requested_text_with_options(cx, codec, source, ReplEvalOptions::default())
}

/// Admits explicitly requested eval with a declared result shape and caps.
pub fn eval_requested_text_with_options(
    cx: &mut Cx,
    codec: &Symbol,
    source: &str,
    options: ReplEvalOptions,
) -> Result<String, String> {
    let expr = decode_eval_expr(cx, codec, source)?;
    let broker = broker(cx).map_err(|err| format!("{err:?}"))?;
    let value = broker
        .admit(
            cx,
            ReadEvalRequest {
                origin: RequestOrigin::new(Symbol::new("repl")),
                codec: codec.clone(),
                source: ReadEvalSource::Expr(expr),
                read_policy: trusted_read_eval_policy(),
                requires: options.requires,
                allow: options.allow,
                expected_shape: options.expected_shape,
            },
        )
        .map_err(|err| format!("{err:?}"))?;
    let expr = value
        .object()
        .as_expr(cx)
        .map_err(|err| format!("{err:?}"))?;
    match encode_with_codec(cx, codec, &expr, EncodeOptions::default())
        .map_err(|err| format!("{err:?}"))?
    {
        Output::Text(text) => Ok(text),
        Output::Bytes(_) => Ok("<bytes>".to_owned()),
    }
}

fn decode_eval_expr(cx: &mut Cx, codec: &Symbol, source: &str) -> Result<Expr, String> {
    match decode_default_with_codec(
        cx,
        codec,
        Input::Text(source.to_owned()),
        ReadPolicy::default(),
        DecodePosition::Eval,
    )
    .map_err(|err| format!("{err:?}"))?
    {
        DecodedForm::Term(term) => Ok(Expr::from(term)),
        DecodedForm::Datum(datum) => Ok(Expr::from(datum)),
    }
}

/// Runs read-eval-print over `reader`, writing one output line per non-empty
/// input line.
pub fn run_repl_lines<R, W>(
    cx: &mut Cx,
    codec: &Symbol,
    reader: R,
    writer: &mut W,
) -> Result<(), String>
where
    R: BufRead,
    W: Write,
{
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read stdin: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match eval_requested_text(cx, codec, trimmed) {
            Ok(result) => writeln!(writer, "{result}"),
            Err(err) => writeln!(writer, "error: {err}"),
        }
        .map_err(|err| format!("write stdout: {err}"))?;
    }
    Ok(())
}

fn trusted_read_eval_policy() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: CapabilitySet::new().grant(read_eval_capability()),
    }
}

fn broker(cx: &mut Cx) -> sim_kernel::Result<ReadEvalBroker> {
    install_read_eval_broker(cx)?;
    let value = cx.resolve_value(&read_eval_broker_symbol())?;
    value
        .object()
        .downcast_ref::<ReadEvalBroker>()
        .cloned()
        .ok_or_else(|| sim_kernel::Error::Eval("read-eval broker value has wrong type".to_owned()))
}

#[cfg(test)]
mod tests {
    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{CapabilityName, Cx, Symbol, macro_expand_eval_capability};
    use sim_lib_core::{ReadEvalOutcome, read_eval_broker_symbol};
    use sim_lib_numbers_prelude::NumbersPreludeLib;
    use sim_shape::{ExprKind, ExprKindShape};

    use super::{ReplEvalOptions, eval_line_for_tests, eval_requested_text, run_repl_lines};

    fn boot() -> Cx {
        let mut cx = sim_test_support::core_cx();
        cx.grant(macro_expand_eval_capability());
        NumbersPreludeLib::new().install_all(&mut cx).unwrap();
        let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
        cx.load_lib(&lisp).unwrap();
        cx
    }

    #[test]
    fn eval_line_for_tests_computes_value() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");

        let result = eval_line_for_tests(&mut cx, &codec, "(math/add (math/mul 6 7) 0)").unwrap();

        assert_eq!(result, "42");
    }

    #[test]
    fn requested_eval_uses_broker_and_records_repl_origin() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");

        let result = eval_requested_text(&mut cx, &codec, "(math/add 1 2)").unwrap();

        assert_eq!(result, "3");
        let broker = cx
            .resolve_value(&read_eval_broker_symbol())
            .unwrap()
            .object()
            .downcast_ref::<sim_lib_core::ReadEvalBroker>()
            .cloned()
            .unwrap();
        let decisions = broker.decisions(&cx).unwrap();
        assert!(decisions.iter().any(|decision| {
            decision.origin.tag == Symbol::new("repl")
                && decision.outcome == ReadEvalOutcome::Admitted
        }));
        assert!(
            decisions
                .iter()
                .any(|decision| { decision.active.contains(&macro_expand_eval_capability()) })
        );
    }

    #[test]
    fn requested_eval_checks_declared_shape() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");

        let err = super::eval_requested_text_with_options(
            &mut cx,
            &codec,
            "(math/add 1 2)",
            ReplEvalOptions {
                expected_shape: std::sync::Arc::new(ExprKindShape::new(ExprKind::String)),
                ..ReplEvalOptions::default()
            },
        )
        .unwrap_err();

        assert!(err.contains("WrongShape"));
    }

    #[test]
    fn requested_eval_checks_required_capabilities() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");
        let required = CapabilityName::new("secret/env");

        let err = super::eval_requested_text_with_options(
            &mut cx,
            &codec,
            "(math/add 1 2)",
            ReplEvalOptions {
                requires: vec![required.clone()],
                ..ReplEvalOptions::default()
            },
        )
        .unwrap_err();

        assert!(err.contains("CapabilityDenied"));
        let broker = cx
            .resolve_value(&read_eval_broker_symbol())
            .unwrap()
            .object()
            .downcast_ref::<sim_lib_core::ReadEvalBroker>()
            .cloned()
            .unwrap();
        let decisions = broker.decisions(&cx).unwrap();
        assert!(decisions.iter().any(|decision| {
            decision.requires == vec![required.clone()]
                && decision.outcome == ReadEvalOutcome::MissingPower
        }));
    }

    #[test]
    fn run_repl_lines_writes_non_empty_results() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");
        let mut output = Vec::new();

        run_repl_lines(
            &mut cx,
            &codec,
            "\n(math/add 1 2)\n".as_bytes(),
            &mut output,
        )
        .unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "3\n");
    }

    #[test]
    fn run_repl_lines_records_repl_admission() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");
        let mut output = Vec::new();

        run_repl_lines(&mut cx, &codec, "(math/add 1 2)\n".as_bytes(), &mut output).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "3\n");
        let broker = cx
            .resolve_value(&read_eval_broker_symbol())
            .unwrap()
            .object()
            .downcast_ref::<sim_lib_core::ReadEvalBroker>()
            .cloned()
            .unwrap();
        let decisions = broker.decisions(&cx).unwrap();
        assert!(decisions.iter().any(|decision| {
            decision.origin.tag == Symbol::new("repl")
                && decision.outcome == ReadEvalOutcome::Admitted
        }));
    }
}
