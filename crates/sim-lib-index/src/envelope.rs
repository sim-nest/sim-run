//! Bootloader envelope decoding for the index entrypoint.

use sim_kernel::{Cx, Error, Expr, Result, Value};

/// Extracts payload arguments from the generic CLI envelope.
pub fn envelope_args(cx: &mut Cx, envelope: &Value) -> Result<Vec<String>> {
    let Some(table) = envelope.object().as_table_impl() else {
        return Err(Error::Eval("index CLI envelope is not a table".to_owned()));
    };
    let value = table.get(cx, sim_kernel::Symbol::new("args"))?;
    let Expr::List(items) = value.object().as_expr(cx)? else {
        return Err(Error::TypeMismatch {
            expected: "argument list",
            found: "non-list",
        });
    };
    items
        .into_iter()
        .map(|item| match item {
            Expr::String(value) => Ok(value),
            _ => Err(Error::TypeMismatch {
                expected: "string argument",
                found: "non-string",
            }),
        })
        .collect()
}
