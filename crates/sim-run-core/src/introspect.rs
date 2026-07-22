use sim_kernel::{Args, ExportKind, ExportRecord, ExportState, LibManifest, Symbol};

use crate::{
    CliBoot, CliError, LibSourceSpec, LoadReceipt, LoadReceiptRole, LoadSession,
    crates_io::{CratesIoListing, CratesIoListingSource, fallback_spec_for_symbol},
    source::symbol_from_text,
};

const CLI_LIST_ENTRYPOINT: &str = "cli/list";
const CLI_INSPECT_ENTRYPOINT: &str = "cli/inspect";

impl LoadSession {
    /// Loads a boot session and returns list/inspect output for loader metadata.
    ///
    /// Listing or inspecting loaded libraries does not require a boot codec. When
    /// the codec is unavailable, fall back to loading just the explicit `--load`
    /// sources so they can still be introspected. (`load_boot` resolves the boot
    /// codec first and returns before loading `--load` sources when that codec is
    /// unavailable, so an empty receipt set means the codec was the failure.)
    pub fn run_loaded_introspection(&mut self, boot: &CliBoot) -> Result<String, CliError> {
        if let Err(codec_err) = self.load_boot(boot) {
            if !self.receipts().is_empty() || boot.loads.is_empty() {
                return Err(codec_err);
            }
            for source in &boot.loads {
                self.load_source(source)?;
            }
        }
        self.loaded_introspection_output(boot)
    }

    pub(crate) fn loaded_introspection_output(
        &mut self,
        boot: &CliBoot,
    ) -> Result<String, CliError> {
        let mut sections = Vec::new();
        if boot.list {
            sections.push(self.list_output()?);
        }
        if let Some(target) = &boot.inspect {
            sections.push(self.inspect_output(target)?);
        }
        Ok(join_sections(sections))
    }

    fn list_output(&mut self) -> Result<String, CliError> {
        if let Some(delegate) = select_delegate(self.receipts(), CLI_LIST_ENTRYPOINT) {
            return self.call_delegate(&delegate, Vec::new());
        }
        Ok(format_list(self))
    }

    fn inspect_output(&mut self, target: &str) -> Result<String, CliError> {
        if let Some(delegate) = select_delegate(self.receipts(), CLI_INSPECT_ENTRYPOINT) {
            let target = self
                .cx_mut()
                .factory()
                .string(target.to_owned())
                .map_err(|err| CliError::new(format!("build inspect target: {err}")))?;
            return self.call_delegate(&delegate, vec![target]);
        }
        self.inspect_target(target)
    }

    fn call_delegate(
        &mut self,
        delegate: &IntrospectionDelegate,
        args: Vec<sim_kernel::Value>,
    ) -> Result<String, CliError> {
        let result = self
            .cx_mut()
            .call_function(&delegate.symbol, Args::new(args))
            .map_err(|err| {
                CliError::new(format!(
                    "introspection delegate {} from {} failed: {err}",
                    delegate.symbol, delegate.lib
                ))
            })?;
        let mut output = result.object().display(self.cx_mut()).map_err(|err| {
            CliError::new(format!("display introspection delegate result: {err}"))
        })?;
        if !output.ends_with('\n') {
            output.push('\n');
        }
        Ok(output)
    }

    fn inspect_target(&mut self, target: &str) -> Result<String, CliError> {
        let symbol = symbol_from_text(target);
        if let Some(receipt) = self
            .receipts()
            .iter()
            .find(|receipt| receipt.manifest.id == symbol)
        {
            return Ok(format_receipt(receipt));
        }

        let export_matches = self
            .receipts()
            .iter()
            .flat_map(|receipt| {
                receipt
                    .exports
                    .iter()
                    .filter(|record| record.symbol == symbol)
                    .map(move |record| (receipt, record))
            })
            .collect::<Vec<_>>();
        if !export_matches.is_empty() {
            return Ok(format_export_matches(target, &export_matches));
        }

        let source = parse_inspect_source(target);
        self.inspect_source_or_fallback(&source)
    }

    fn inspect_source_or_fallback(&mut self, source: &LibSourceSpec) -> Result<String, CliError> {
        match self.inspect_source(source) {
            Ok(report) => Ok(report),
            Err(err) => match source {
                LibSourceSpec::Symbol(symbol) => {
                    let Some(spec) = fallback_spec_for_symbol(symbol) else {
                        return Err(err);
                    };
                    self.inspect_source(&LibSourceSpec::CratesIo(spec))
                }
                _ => Err(err),
            },
        }
    }

    fn inspect_source(&mut self, source: &LibSourceSpec) -> Result<String, CliError> {
        match source {
            LibSourceSpec::Host(name) => {
                let lib = self.hosts().instantiate(name, self.config_state())?;
                Ok(format_manifest_source(source, source, &lib.manifest()))
            }
            LibSourceSpec::CratesIo(spec) => {
                let resolved = self.crates_io().resolve(spec)?;
                let resolved_source = LibSourceSpec::Path(resolved.artifact.clone());
                let manifest = self.inspect_data_source_manifest(
                    sim_run_loaders::path_source_spec(resolved.artifact),
                )?;
                Ok(format_manifest_source(source, &resolved_source, &manifest))
            }
            _ => {
                let requested = source
                    .to_kernel_data_source()
                    .ok_or_else(|| CliError::new(format!("cannot inspect source {source}")))?;
                let resolved = self.resolve_data_source(requested.clone());
                let manifest = self.inspect_data_source_manifest(resolved.clone())?;
                Ok(format_manifest_source(
                    &LibSourceSpec::from_kernel_data_source(requested),
                    &LibSourceSpec::from_kernel_data_source(resolved),
                    &manifest,
                ))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IntrospectionDelegate {
    lib: Symbol,
    symbol: Symbol,
}

fn select_delegate(receipts: &[LoadReceipt], name: &str) -> Option<IntrospectionDelegate> {
    receipts
        .iter()
        .filter(|receipt| matches!(receipt.role, LoadReceiptRole::Library))
        .find_map(|receipt| delegate_for_receipt(receipt, name))
        .or_else(|| {
            receipts
                .iter()
                .filter(|receipt| matches!(receipt.role, LoadReceiptRole::BootCodec { .. }))
                .find_map(|receipt| delegate_for_receipt(receipt, name))
        })
}

fn delegate_for_receipt(receipt: &LoadReceipt, name: &str) -> Option<IntrospectionDelegate> {
    receipt
        .exports
        .iter()
        .find(|record| record_is_resolved_function(record, name))
        .map(|record| IntrospectionDelegate {
            lib: receipt.manifest.id.clone(),
            symbol: record.symbol.clone(),
        })
}

fn record_is_resolved_function(record: &ExportRecord, name: &str) -> bool {
    record.kind == ExportKind::named(ExportKind::FUNCTION)
        && matches!(record.state, ExportState::Resolved { .. })
        && record.symbol.as_qualified_str() == name
}

fn format_list(session: &LoadSession) -> String {
    let mut output = String::new();
    output.push_str("catalog sources:\n");
    if session.catalog_sources().is_empty() {
        output.push_str("- none\n");
    } else {
        for (symbol, source) in session.catalog_sources() {
            output.push_str(&format!("- symbol:{symbol} -> {source}\n"));
        }
    }

    output.push_str("crates.io artifacts:\n");
    match session.crates_io().available_artifacts() {
        Ok(listings) if listings.is_empty() => output.push_str("- none\n"),
        Ok(listings) => {
            for listing in listings {
                output.push_str(&format_crates_listing(&listing));
            }
        }
        Err(err) => output.push_str(&format!("- unavailable: {err}\n")),
    }

    output.push_str("loaded libs:\n");
    if session.receipts().is_empty() {
        output.push_str("- none\n");
    } else {
        for receipt in session.receipts() {
            output.push_str(&format!(
                "- role={} lib={} version={} requested={} resolved={} exports={}\n",
                role_label(&receipt.role),
                receipt.manifest.id,
                receipt.manifest.version.0,
                receipt.requested_source,
                receipt.resolved_source,
                receipt.exports.len()
            ));
        }
    }
    output
}

fn format_crates_listing(listing: &CratesIoListing) -> String {
    format!(
        "- crates.io:{}@{} source={} artifact={}\n",
        listing.package,
        listing.version,
        crates_listing_source_label(&listing.source),
        listing.artifact.display()
    )
}

fn crates_listing_source_label(source: &CratesIoListingSource) -> &'static str {
    match source {
        CratesIoListingSource::Cache => "cache",
        CratesIoListingSource::Registry => "registry",
    }
}

fn format_receipt(receipt: &LoadReceipt) -> String {
    let mut output = String::new();
    output.push_str(&format!("lib {}\n", receipt.manifest.id));
    output.push_str(&format!("version {}\n", receipt.manifest.version.0));
    output.push_str(&format!("target {:?}\n", receipt.manifest.target));
    output.push_str(&format!("role {}\n", role_label(&receipt.role)));
    output.push_str(&format!("requested {}\n", receipt.requested_source));
    output.push_str(&format!("resolved {}\n", receipt.resolved_source));
    output.push_str(&format_dependencies(receipt));
    output.push_str(&format_exports(&receipt.exports));
    output
}

fn format_manifest_source(
    requested: &LibSourceSpec,
    resolved: &LibSourceSpec,
    manifest: &LibManifest,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("source {}\n", requested));
    output.push_str(&format!("resolved {}\n", resolved));
    output.push_str(&format!("lib {}\n", manifest.id));
    output.push_str(&format!("version {}\n", manifest.version.0));
    output.push_str(&format!("target {:?}\n", manifest.target));
    output.push_str("exports:\n");
    for record in manifest.declared_export_records() {
        output.push_str(&format_export(&record));
    }
    if manifest.exports.is_empty() {
        output.push_str("- none\n");
    }
    output
}

fn format_export_matches(target: &str, matches: &[(&LoadReceipt, &ExportRecord)]) -> String {
    let mut output = String::new();
    output.push_str(&format!("export {target}\n"));
    for (receipt, record) in matches {
        output.push_str(&format!("- lib={} ", receipt.manifest.id));
        output.push_str(format_export(record).trim_start_matches("- "));
    }
    output
}

fn format_dependencies(receipt: &LoadReceipt) -> String {
    let mut output = String::new();
    output.push_str("dependencies:\n");
    if receipt.dependencies.is_empty() {
        output.push_str("- none\n");
    } else {
        for dependency in &receipt.dependencies {
            output.push_str(&format!(
                "- lib_id={} symbol={}\n",
                dependency.lib_id.0, dependency.symbol
            ));
        }
    }
    output
}

fn format_exports(exports: &[ExportRecord]) -> String {
    let mut output = String::new();
    output.push_str("exports:\n");
    if exports.is_empty() {
        output.push_str("- none\n");
    } else {
        for record in exports {
            output.push_str(&format_export(record));
        }
    }
    output
}

fn format_export(record: &ExportRecord) -> String {
    format!(
        "- kind={} symbol={} state={}\n",
        record.kind.symbol(),
        record.symbol,
        state_label(&record.state)
    )
}

fn state_label(state: &ExportState) -> String {
    match state {
        ExportState::Resolved { id } => format!("resolved:{id:?}"),
        ExportState::Declared => "declared".to_owned(),
        ExportState::Unsupported { reason } => format!("unsupported:{reason}"),
        ExportState::Invalid { error } => format!("invalid:{error}"),
    }
}

fn role_label(role: &LoadReceiptRole) -> String {
    match role {
        LoadReceiptRole::Library => "library".to_owned(),
        LoadReceiptRole::BootCodec { name, symbol } => format!("boot-codec:{name}:{symbol}"),
    }
}

fn parse_inspect_source(target: &str) -> LibSourceSpec {
    target
        .parse::<LibSourceSpec>()
        .unwrap_or_else(|_| LibSourceSpec::Symbol(target.to_owned()))
}

fn join_sections(sections: Vec<String>) -> String {
    let mut output = sections
        .into_iter()
        .map(|mut section| {
            if !section.ends_with('\n') {
                section.push('\n');
            }
            section
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}
