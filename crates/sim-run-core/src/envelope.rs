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
            cx.factory()
                .string(arg.to_string_lossy().into_owned())
                .map_err(envelope_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let args = cx.factory().list(args).map_err(envelope_error)?;
    let eval = option_string(cx, envelope.eval.as_deref())?;
    let script = option_string(
        cx,
        envelope
            .script
            .as_ref()
            .map(|path| path.to_string_lossy())
            .as_deref(),
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

fn envelope_error(err: sim_kernel::Error) -> CliError {
    CliError::new(format!("build CLI envelope value: {err}"))
}
