use std::io::{BufRead, Write};

use sim_codec::{
    DecodePosition, DecodedForm, Input, Output, decode_default_with_codec, encode_with_codec,
};
use sim_kernel::{Cx, EncodeOptions, Expr, ReadPolicy, Symbol};

/// Decodes, evaluates, and re-encodes one source line through `codec`.
pub fn eval_line(cx: &mut Cx, codec: &Symbol, line: &str) -> Result<String, String> {
    let decoded = match decode_default_with_codec(
        cx,
        codec,
        Input::Text(line.to_owned()),
        ReadPolicy::default(),
        DecodePosition::Eval,
    )
    .map_err(|err| format!("{err:?}"))?
    {
        DecodedForm::Term(term) => Expr::from(term),
        DecodedForm::Datum(datum) => Expr::from(datum),
    };
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
        match eval_line(cx, codec, trimmed) {
            Ok(result) => writeln!(writer, "{result}"),
            Err(err) => writeln!(writer, "error: {err}"),
        }
        .map_err(|err| format!("write stdout: {err}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{Cx, Symbol};
    use sim_lib_numbers_prelude::NumbersPreludeLib;

    use super::{eval_line, run_repl_lines};

    fn boot() -> Cx {
        let mut cx = sim_test_support::core_cx();
        NumbersPreludeLib::new().install_all(&mut cx).unwrap();
        let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
        cx.load_lib(&lisp).unwrap();
        cx
    }

    #[test]
    fn eval_line_computes_value() {
        let mut cx = boot();
        let codec = Symbol::qualified("codec", "lisp");

        let result = eval_line(&mut cx, &codec, "(math/add (math/mul 6 7) 0)").unwrap();

        assert_eq!(result, "42");
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
}
