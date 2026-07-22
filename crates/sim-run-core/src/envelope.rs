use std::ffi::OsStr;

use sim_kernel::{Cx, Symbol, Value};

use crate::{CliEnvelope, CliError, source::symbol_from_text};

/// Projects the command envelope into a kernel table value.
pub fn cli_envelope_value(cx: &mut Cx, envelope: &CliEnvelope) -> Result<Value, CliError> {
    let codec = cx
        .factory()
        .symbol(symbol_from_text(&envelope.codec))
        .map_err(envelope_error)?;
    let verb = option_string(cx, envelope.verb.as_deref())?;
    let args = envelope
        .args
        .iter()
        .map(|arg| {
            let arg = os_str_text(arg.as_os_str(), "CLI argument")?;
            cx.factory().string(arg.to_owned()).map_err(envelope_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let args = cx.factory().list(args).map_err(envelope_error)?;
    let eval = option_string(cx, envelope.eval.as_deref())?;
    let script = option_os_string(
        cx,
        envelope.script.as_ref().map(|path| path.as_os_str()),
        "script path",
    )?;
    let stdin = option_string(cx, envelope.stdin.as_deref())?;

    cx.factory()
        .table(vec![
            (Symbol::new("codec"), codec),
            (Symbol::new("verb"), verb),
            (Symbol::new("args"), args),
            (Symbol::new("eval"), eval),
            (Symbol::new("script"), script),
            (Symbol::new("stdin"), stdin),
        ])
        .map_err(envelope_error)
}

fn option_string(cx: &mut Cx, value: Option<&str>) -> Result<Value, CliError> {
    match value {
        Some(value) => cx
            .factory()
            .string(value.to_owned())
            .map_err(envelope_error),
        None => cx.factory().nil().map_err(envelope_error),
    }
}

fn option_os_string(cx: &mut Cx, value: Option<&OsStr>, context: &str) -> Result<Value, CliError> {
    match value {
        Some(value) => cx
            .factory()
            .string(os_str_text(value, context)?.to_owned())
            .map_err(envelope_error),
        None => cx.factory().nil().map_err(envelope_error),
    }
}

fn os_str_text<'a>(value: &'a OsStr, context: &str) -> Result<&'a str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError::new(format!("{context} requires UTF-8 text")))
}

fn envelope_error(err: sim_kernel::Error) -> CliError {
    CliError::new(format!("build CLI envelope value: {err}"))
}

#[cfg(test)]
mod tests {
    use sim_kernel::testing::bare_cx as cx;

    use super::*;
    use crate::CliEnvelope;

    #[cfg(unix)]
    #[test]
    fn envelope_rejects_non_utf8_script_path() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

        let mut cx = cx();
        let envelope = CliEnvelope {
            codec: "codec/lisp".to_owned(),
            verb: None,
            args: Vec::new(),
            eval: None,
            script: Some(PathBuf::from(OsString::from_vec(
                b"/tmp/sim-run-\xff-script.sim".to_vec(),
            ))),
            stdin: None,
        };

        let err = cli_envelope_value(&mut cx, &envelope).unwrap_err();

        assert_eq!(err.to_string(), "script path requires UTF-8 text");
    }
}
