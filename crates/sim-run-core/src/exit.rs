use sim_kernel::{Cx, Expr, Value};

use crate::CliError;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;

/// Converts a loaded entrypoint result into a process exit code.
pub fn value_to_exit_code(cx: &mut Cx, value: Value) -> Result<i32, CliError> {
    if let Ok(Expr::Number(number)) = value.object().as_expr(cx)
        && number.domain == sim_kernel::Symbol::new("exit-code")
    {
        return number
            .canonical
            .parse::<i32>()
            .ok()
            .filter(|code| (0..=255).contains(code))
            .ok_or_else(|| CliError::new("CLI exit-code number must be an integer in 0..=255"));
    }
    value
        .object()
        .truth(cx)
        .map(|success| if success { EXIT_SUCCESS } else { EXIT_FAILURE })
        .map_err(|err| CliError::new(format!("convert CLI result to exit code: {err}")))
}

#[cfg(test)]
mod tests {
    use super::value_to_exit_code;
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
    use std::sync::Arc;

    #[test]
    fn explicit_exit_code_domain_preserves_product_outcome() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let value = cx
            .factory()
            .number_literal(Symbol::new("exit-code"), "30".into())
            .unwrap();
        assert_eq!(value_to_exit_code(&mut cx, value).unwrap(), 30);
    }
}
