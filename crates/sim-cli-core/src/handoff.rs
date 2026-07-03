use sim_kernel::{Args, Cx, ExportKind, ExportRecord, ExportState, Symbol};

use crate::{
    CliBoot, CliEnvelope, CliError, LoadReceipt, LoadReceiptRole, LoadSession,
    envelope::cli_envelope_value, exit::value_to_exit_code,
};

/// Symbol prefix a loaded lib claims to own command-line execution.
pub const CLI_MAIN_ENTRYPOINT: &str = "cli/main";

/// A loaded function that owns command-line execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliEntrypoint {
    /// Library that exported the entrypoint.
    pub lib: Symbol,
    /// Exported function symbol invoked for the handoff.
    pub symbol: Symbol,
}

impl LoadSession {
    /// Loads a boot session and runs the selected loaded CLI entrypoint.
    pub fn run_loaded_boot(&mut self, boot: &CliBoot) -> Result<i32, CliError> {
        if boot.list || boot.inspect.is_some() {
            print!("{}", self.run_loaded_introspection(boot)?);
            return Ok(0);
        }
        self.load_boot(boot)?;
        self.run_loaded_handoff(&boot.envelope())
    }

    /// Runs the selected loaded CLI entrypoint for an already-loaded session.
    pub fn run_loaded_handoff(&mut self, envelope: &CliEnvelope) -> Result<i32, CliError> {
        let entrypoint = select_cli_entrypoint(self.receipts())?;
        run_loaded_cli(self.cx_mut(), &entrypoint, envelope)
    }
}

/// Builds the qualified `cli/main/NAME` entrypoint symbol for a named lib.
pub fn cli_main_entrypoint_symbol(name: &str) -> Symbol {
    Symbol::qualified("cli", format!("main/{name}"))
}

/// Selects the loaded entrypoint that claims [`CLI_MAIN_ENTRYPOINT`].
///
/// Prefers a `--load` library over the boot codec, and returns an error
/// when no loaded lib claims the entrypoint.
pub fn select_cli_entrypoint(receipts: &[LoadReceipt]) -> Result<CliEntrypoint, CliError> {
    receipts
        .iter()
        .filter(|receipt| matches!(receipt.role, LoadReceiptRole::Library))
        .find_map(entrypoint_for_receipt)
        .or_else(|| {
            receipts
                .iter()
                .filter(|receipt| matches!(receipt.role, LoadReceiptRole::BootCodec { .. }))
                .find_map(entrypoint_for_receipt)
        })
        .ok_or_else(|| no_entrypoint_error(receipts))
}

/// Calls a loaded entrypoint with the boot envelope and returns its exit code.
pub fn run_loaded_cli(
    cx: &mut Cx,
    entrypoint: &CliEntrypoint,
    envelope: &CliEnvelope,
) -> Result<i32, CliError> {
    let envelope = cli_envelope_value(cx, envelope)?;
    let result = cx
        .call_function(&entrypoint.symbol, Args::new(vec![envelope]))
        .map_err(|err| {
            CliError::new(format!(
                "cli handoff failed for {} from {}: {err}",
                entrypoint.symbol, entrypoint.lib
            ))
        })?;
    value_to_exit_code(cx, result)
}

fn entrypoint_for_receipt(receipt: &LoadReceipt) -> Option<CliEntrypoint> {
    receipt
        .exports
        .iter()
        .find(|record| record_claims_cli_main(record))
        .map(|record| CliEntrypoint {
            lib: receipt.manifest.id.clone(),
            symbol: record.symbol.clone(),
        })
}

fn record_claims_cli_main(record: &ExportRecord) -> bool {
    record.kind == ExportKind::named(ExportKind::FUNCTION)
        && matches!(record.state, ExportState::Resolved { .. })
        && symbol_claims_cli_main(&record.symbol)
}

fn symbol_claims_cli_main(symbol: &Symbol) -> bool {
    let symbol = symbol.as_qualified_str();
    symbol == CLI_MAIN_ENTRYPOINT || symbol.starts_with(&format!("{CLI_MAIN_ENTRYPOINT}/"))
}

fn no_entrypoint_error(receipts: &[LoadReceipt]) -> CliError {
    let loaded = if receipts.is_empty() {
        "none".to_owned()
    } else {
        receipts
            .iter()
            .map(|receipt| receipt.manifest.id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    CliError::new(format!(
        "no loaded lib claims {CLI_MAIN_ENTRYPOINT}; loaded libs: {loaded}; load one with --load"
    ))
}
