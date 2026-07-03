use sim_kernel::{Cx, Value};

use crate::CliError;

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;

/// Converts a loaded entrypoint result into a process exit code.
pub fn value_to_exit_code(cx: &mut Cx, value: Value) -> Result<i32, CliError> {
    value
        .object()
        .truth(cx)
        .map(|success| if success { EXIT_SUCCESS } else { EXIT_FAILURE })
        .map_err(|err| CliError::new(format!("convert CLI result to exit code: {err}")))
}
