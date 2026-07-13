//! Loaded-state and effective-config reports.

use sim_codec_config::ConfigEncoder;
use sim_config::{
    ConfigProbeReport, ConfigProbeStatus, ConfigSecretField, ConfigSource, EffectiveConfig,
    ProbeMode,
};
use sim_kernel::{Expr, Symbol};

use crate::{CliBoot, CliError, LoadReceipt, LoadReceiptRole, LoadSession};

/// One loaded library as it appears in a boot session report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedLibReport {
    /// Role assigned by the bootloader.
    pub role: String,
    /// Library identifier from the manifest.
    pub lib: Symbol,
    /// Library version from the manifest.
    pub version: String,
    /// Source requested by the operator or bootloader.
    pub requested: String,
    /// Concrete source used by the loader after catalog or registry resolution.
    pub resolved: String,
    /// Number of export records published by the loaded library.
    pub exports: usize,
}

impl LoadedLibReport {
    /// Builds a loaded-library report row from a load receipt.
    pub fn from_receipt(receipt: &LoadReceipt) -> Self {
        Self {
            role: role_label(&receipt.role).to_owned(),
            lib: receipt.manifest.id.clone(),
            version: receipt.manifest.version.0.clone(),
            requested: receipt.requested_source.to_string(),
            resolved: receipt.resolved_source.to_string(),
            exports: receipt.exports.len(),
        }
    }
}

/// Discovery status for one config source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceStatus {
    /// The source existed and produced a config layer.
    Found,
    /// The source was checked but was absent.
    Missing,
    /// The source was known but skipped by source-selection policy.
    Ignored,
    /// The source existed but could not decode or normalize as config.
    Rejected,
}

/// One config source and its discovery status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigSourceReport {
    /// Source descriptor that was checked.
    pub source: ConfigSource,
    /// Discovery status for the source.
    pub status: SourceStatus,
}

/// Request selected by the `sim config ...` CLI surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigReportRequest {
    /// Report kind to render.
    pub kind: ConfigReportKind,
    /// Whether the report should render as stable JSON.
    pub json: bool,
}

/// Config report variants supported by the bootloader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigReportKind {
    /// Loaded libraries, sources, probes, and diagnostics.
    Status,
    /// Effective config table for one library.
    Effective {
        /// Library whose effective table should be rendered.
        lib: Symbol,
    },
    /// Source provenance and diagnostics only.
    Sources,
}

/// Loaded-state report built from one [`LoadSession`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedStateReport {
    /// Loaded libraries in receipt order.
    pub libs: Vec<LoadedLibReport>,
    /// Config source status rows in discovery order.
    pub config_sources: Vec<ConfigSourceReport>,
    /// Effective merged configuration.
    pub effective: EffectiveConfig,
    /// Secret-bearing config fields to redact in report renderers.
    pub secret_fields: Vec<ConfigSecretField>,
    /// Typed probe report records in discovery order.
    pub probe_reports: Vec<ConfigProbeReport>,
    /// Non-fatal config discovery diagnostics.
    pub diagnostics: Vec<String>,
}

impl LoadedStateReport {
    /// Builds a report snapshot from a load session.
    pub fn from_session(session: &LoadSession) -> Self {
        Self {
            libs: session
                .receipts()
                .iter()
                .map(LoadedLibReport::from_receipt)
                .collect(),
            config_sources: session.config_state().source_reports().to_vec(),
            effective: session.config_state().effective().clone(),
            secret_fields: session.config_state().secret_fields().to_vec(),
            probe_reports: session.config_state().probe_reports().to_vec(),
            diagnostics: session.config_state().diagnostics().to_vec(),
        }
    }
}

impl LoadSession {
    /// Loads enough boot state to render a `sim config ...` report.
    pub fn run_config_report(&mut self, boot: &CliBoot) -> Result<String, CliError> {
        let request = boot
            .config_report
            .as_ref()
            .ok_or_else(|| CliError::new("missing config report request"))?;
        self.load_for_config_report(boot)?;
        let report = LoadedStateReport::from_session(self);
        Ok(render_config_report(&report, request))
    }

    fn load_for_config_report(&mut self, boot: &CliBoot) -> Result<(), CliError> {
        match self.load_boot(boot).map(|_| ()) {
            Ok(()) => Ok(()),
            Err(err) => {
                if self.receipts().is_empty() {
                    if boot.loads.is_empty() {
                        return Ok(());
                    }
                    for source in &boot.loads {
                        self.load_source(source)?;
                    }
                    return Ok(());
                }
                if is_no_codec_error(&err) {
                    return Ok(());
                }
                Err(err)
            }
        }
    }
}

/// Renders the selected config report.
pub fn render_config_report(report: &LoadedStateReport, request: &ConfigReportRequest) -> String {
    match (&request.kind, request.json) {
        (ConfigReportKind::Status, false) => format_config_status(report),
        (ConfigReportKind::Status, true) => format_config_status_json(report),
        (ConfigReportKind::Sources, false) => format_config_sources(report),
        (ConfigReportKind::Sources, true) => format_config_sources_json(report),
        (ConfigReportKind::Effective { lib }, false) => format_effective_config(report, lib),
        (ConfigReportKind::Effective { lib }, true) => format_effective_config_json(report, lib),
    }
}

/// Renders a text status report.
pub fn format_config_status(report: &LoadedStateReport) -> String {
    let mut output = String::new();
    push_loaded_libs(&mut output, &report.libs);
    push_config_sources(&mut output, &report.config_sources);
    push_probe_reports(&mut output, &report.probe_reports);
    push_diagnostics(&mut output, &report.diagnostics);
    output
}

/// Renders a stable JSON status report.
pub fn format_config_status_json(report: &LoadedStateReport) -> String {
    let mut output = String::new();
    output.push('{');
    output.push_str("\"libs\":");
    push_loaded_libs_json(&mut output, &report.libs);
    output.push_str(",\"config_sources\":");
    push_config_sources_json(&mut output, &report.config_sources);
    output.push_str(",\"effective\":");
    push_effective_json(&mut output, report);
    output.push_str(",\"probes\":");
    push_probe_reports_json(&mut output, &report.probe_reports);
    output.push_str(",\"diagnostics\":");
    push_strings_json(&mut output, &report.diagnostics);
    output.push_str("}\n");
    output
}

/// Renders a text config-source report.
pub fn format_config_sources(report: &LoadedStateReport) -> String {
    let mut output = String::new();
    push_config_sources(&mut output, &report.config_sources);
    push_diagnostics(&mut output, &report.diagnostics);
    output
}

/// Renders a stable JSON config-source report.
pub fn format_config_sources_json(report: &LoadedStateReport) -> String {
    let mut output = String::new();
    output.push('{');
    output.push_str("\"config_sources\":");
    push_config_sources_json(&mut output, &report.config_sources);
    output.push_str(",\"diagnostics\":");
    push_strings_json(&mut output, &report.diagnostics);
    output.push_str("}\n");
    output
}

/// Renders one effective config table as config text.
pub fn format_effective_config(report: &LoadedStateReport, lib: &Symbol) -> String {
    let mut output = String::new();
    output.push_str(&format!("lib {lib}\n"));
    match report.effective.dir.table(lib) {
        Some(table) => {
            let redacted = redact_table_expr(lib, &table.table, &report.secret_fields);
            match ConfigEncoder::new().encode_text(&redacted) {
                Ok(text) if !text.is_empty() => output.push_str(&text),
                Ok(_) => output.push_str("- empty\n"),
                Err(err) => output.push_str(&format!("- unencodable config: {err}\n")),
            }
        }
        None => output.push_str("- no effective config\n"),
    }
    output
}

/// Renders one effective config table as stable JSON.
pub fn format_effective_config_json(report: &LoadedStateReport, lib: &Symbol) -> String {
    let mut output = String::new();
    output.push('{');
    output.push_str("\"lib\":");
    push_json_string(&mut output, &lib.as_qualified_str());
    output.push_str(",\"table\":");
    if let Some(table) = report.effective.dir.table(lib) {
        let redacted = redact_table_expr(lib, &table.table, &report.secret_fields);
        push_expr_json(&mut output, &redacted);
    } else {
        output.push_str("null");
    }
    output.push_str("}\n");
    output
}

fn push_loaded_libs(output: &mut String, libs: &[LoadedLibReport]) {
    output.push_str("loaded libs:\n");
    if libs.is_empty() {
        output.push_str("- none\n");
        return;
    }
    for lib in libs {
        output.push_str(&format!(
            "- role={} lib={} version={} requested={} resolved={} exports={}\n",
            lib.role, lib.lib, lib.version, lib.requested, lib.resolved, lib.exports
        ));
    }
}

fn push_config_sources(output: &mut String, sources: &[ConfigSourceReport]) {
    output.push_str("config sources:\n");
    if sources.is_empty() {
        output.push_str("- none\n");
        return;
    }
    for source in sources {
        output.push_str(&format!(
            "- source={} status={}\n",
            source_label(&source.source),
            status_label(source.status)
        ));
    }
}

fn push_probe_reports(output: &mut String, probes: &[ConfigProbeReport]) {
    output.push_str("probes:\n");
    if probes.is_empty() {
        output.push_str("- none\n");
        return;
    }
    for probe in probes {
        output.push_str(&format!(
            "- probe={} lib={} mode={} status={}",
            probe.probe,
            probe.lib,
            mode_label(probe.mode),
            probe_status_label(&probe.status)
        ));
        match &probe.status {
            ConfigProbeStatus::Applied => {}
            ConfigProbeStatus::Skipped { reason } => {
                output.push_str(&format!(" reason={reason}"));
            }
            ConfigProbeStatus::Denied { capability } => {
                output.push_str(&format!(" capability={capability}"));
            }
            ConfigProbeStatus::Failed { message } => {
                output.push_str(&format!(" message={message}"));
            }
        }
        output.push_str(&format!(
            " emitted={}\n",
            emitted_keys_label(&probe.emitted_keys)
        ));
    }
}

fn push_diagnostics(output: &mut String, diagnostics: &[String]) {
    if diagnostics.is_empty() {
        return;
    }
    output.push_str("diagnostics:\n");
    for diagnostic in diagnostics {
        output.push_str(&format!("- {diagnostic}\n"));
    }
}

fn push_loaded_libs_json(output: &mut String, libs: &[LoadedLibReport]) {
    output.push('[');
    for (index, lib) in libs.iter().enumerate() {
        comma(output, index);
        output.push('{');
        output.push_str("\"role\":");
        push_json_string(output, &lib.role);
        output.push_str(",\"lib\":");
        push_json_string(output, &lib.lib.as_qualified_str());
        output.push_str(",\"version\":");
        push_json_string(output, &lib.version);
        output.push_str(",\"requested\":");
        push_json_string(output, &lib.requested);
        output.push_str(",\"resolved\":");
        push_json_string(output, &lib.resolved);
        output.push_str(",\"exports\":");
        output.push_str(&lib.exports.to_string());
        output.push('}');
    }
    output.push(']');
}

fn push_config_sources_json(output: &mut String, sources: &[ConfigSourceReport]) {
    output.push('[');
    for (index, source) in sources.iter().enumerate() {
        comma(output, index);
        output.push('{');
        output.push_str("\"source\":");
        push_json_string(output, &source_label(&source.source));
        output.push_str(",\"status\":");
        push_json_string(output, status_label(source.status));
        output.push('}');
    }
    output.push(']');
}

fn push_probe_reports_json(output: &mut String, probes: &[ConfigProbeReport]) {
    output.push('[');
    for (index, probe) in probes.iter().enumerate() {
        comma(output, index);
        output.push('{');
        output.push_str("\"probe\":");
        push_json_string(output, &probe.probe.as_qualified_str());
        output.push_str(",\"lib\":");
        push_json_string(output, &probe.lib.as_qualified_str());
        output.push_str(",\"mode\":");
        push_json_string(output, mode_label(probe.mode));
        output.push_str(",\"status\":");
        push_json_string(output, probe_status_label(&probe.status));
        match &probe.status {
            ConfigProbeStatus::Applied => {}
            ConfigProbeStatus::Skipped { reason } => {
                output.push_str(",\"reason\":");
                push_json_string(output, reason);
            }
            ConfigProbeStatus::Denied { capability } => {
                output.push_str(",\"capability\":");
                push_json_string(output, capability);
            }
            ConfigProbeStatus::Failed { message } => {
                output.push_str(",\"message\":");
                push_json_string(output, message);
            }
        }
        output.push_str(",\"emitted_keys\":");
        push_strings_json(output, &probe.emitted_keys);
        output.push('}');
    }
    output.push(']');
}

fn push_effective_json(output: &mut String, report: &LoadedStateReport) {
    output.push('{');
    for (index, table) in report.effective.dir.entries.iter().enumerate() {
        comma(output, index);
        push_json_string(output, &table.lib.as_qualified_str());
        output.push(':');
        let redacted = redact_table_expr(&table.lib, &table.table, &report.secret_fields);
        push_expr_json(output, &redacted);
    }
    output.push('}');
}

fn push_strings_json(output: &mut String, values: &[String]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        comma(output, index);
        push_json_string(output, value);
    }
    output.push(']');
}

fn push_expr_json(output: &mut String, expr: &Expr) {
    match expr {
        Expr::Nil => output.push_str("null"),
        Expr::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Expr::Number(number) if is_json_number(&number.canonical) => {
            output.push_str(&number.canonical);
        }
        Expr::Number(number) => push_json_string(output, &number.canonical),
        Expr::Symbol(symbol) | Expr::Local(symbol) => {
            push_json_string(output, &symbol.as_qualified_str());
        }
        Expr::String(value) => push_json_string(output, value),
        Expr::Bytes(bytes) => push_json_string(output, &hex_bytes(bytes)),
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                comma(output, index);
                push_expr_json(output, item);
            }
            output.push(']');
        }
        Expr::Map(entries) => {
            output.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                comma(output, index);
                push_json_string(output, &expr_key_label(key));
                output.push(':');
                push_expr_json(output, value);
            }
            output.push('}');
        }
        Expr::Call { operator, args } => {
            output.push_str("{\"$expr\":\"call\",\"operator\":");
            push_expr_json(output, operator);
            output.push_str(",\"args\":");
            push_expr_array_json(output, args);
            output.push('}');
        }
        Expr::Infix {
            operator,
            left,
            right,
        } => {
            output.push_str("{\"$expr\":\"infix\",\"operator\":");
            push_json_string(output, &operator.as_qualified_str());
            output.push_str(",\"left\":");
            push_expr_json(output, left);
            output.push_str(",\"right\":");
            push_expr_json(output, right);
            output.push('}');
        }
        Expr::Prefix { operator, arg } => {
            output.push_str("{\"$expr\":\"prefix\",\"operator\":");
            push_json_string(output, &operator.as_qualified_str());
            output.push_str(",\"arg\":");
            push_expr_json(output, arg);
            output.push('}');
        }
        Expr::Postfix { operator, arg } => {
            output.push_str("{\"$expr\":\"postfix\",\"operator\":");
            push_json_string(output, &operator.as_qualified_str());
            output.push_str(",\"arg\":");
            push_expr_json(output, arg);
            output.push('}');
        }
        Expr::Quote { mode, expr } => {
            output.push_str("{\"$expr\":\"quote\",\"mode\":");
            push_json_string(output, &format!("{mode:?}"));
            output.push_str(",\"expr\":");
            push_expr_json(output, expr);
            output.push('}');
        }
        Expr::Annotated { expr, annotations } => {
            output.push_str("{\"$expr\":\"annotated\",\"expr\":");
            push_expr_json(output, expr);
            output.push_str(",\"annotations\":{");
            for (index, (key, value)) in annotations.iter().enumerate() {
                comma(output, index);
                push_json_string(output, &key.as_qualified_str());
                output.push(':');
                push_expr_json(output, value);
            }
            output.push_str("}}");
        }
        Expr::Extension { tag, payload } => {
            output.push_str("{\"$expr\":\"extension\",\"tag\":");
            push_json_string(output, &tag.as_qualified_str());
            output.push_str(",\"payload\":");
            push_expr_json(output, payload);
            output.push('}');
        }
    }
}

fn push_expr_array_json(output: &mut String, values: &[Expr]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        comma(output, index);
        push_expr_json(output, value);
    }
    output.push(']');
}

fn redact_table_expr(lib: &Symbol, expr: &Expr, secrets: &[ConfigSecretField]) -> Expr {
    let Expr::Map(entries) = expr else {
        return expr.clone();
    };
    Expr::Map(
        entries
            .iter()
            .map(|(key, value)| {
                let key_label = expr_key_label(key);
                if secret_key(lib, &key_label, secrets) {
                    (key.clone(), Expr::String("[redacted]".to_owned()))
                } else {
                    (key.clone(), value.clone())
                }
            })
            .collect(),
    )
}

fn secret_key(lib: &Symbol, key: &str, secrets: &[ConfigSecretField]) -> bool {
    secrets
        .iter()
        .any(|secret| &secret.lib == lib && secret.key == key)
        || key_suggests_secret(key)
}

fn key_suggests_secret(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "password"
        || key == "token"
        || key == "secret"
        || key == "api_key"
        || key.ends_with("_password")
        || key.ends_with("_token")
        || key.ends_with("_secret")
}

fn source_label(source: &ConfigSource) -> String {
    match source {
        ConfigSource::BuiltIn { lib } => format!("built-in:{}", lib.as_qualified_str()),
        ConfigSource::Probe { probe, mode } => {
            format!("probe:{}:{}", probe.as_qualified_str(), mode_label(*mode))
        }
        ConfigSource::HomeFile { path } => format!("home-file:{}", path.display()),
        ConfigSource::WorkFile { path } => format!("work-file:{}", path.display()),
        ConfigSource::SingleFile { path } => format!("single-file:{}", path.display()),
        ConfigSource::Site { site } => format!("site:{}", site.as_qualified_str()),
        ConfigSource::Explicit { label } => format!("explicit:{label}"),
    }
}

fn mode_label(mode: ProbeMode) -> &'static str {
    match mode {
        ProbeMode::Modeled => "modeled",
        ProbeMode::Real => "real",
    }
}

fn probe_status_label(status: &ConfigProbeStatus) -> &'static str {
    match status {
        ConfigProbeStatus::Applied => "applied",
        ConfigProbeStatus::Skipped { .. } => "skipped",
        ConfigProbeStatus::Denied { .. } => "denied",
        ConfigProbeStatus::Failed { .. } => "failed",
    }
}

fn emitted_keys_label(keys: &[String]) -> String {
    if keys.is_empty() {
        "-".to_owned()
    } else {
        keys.join(",")
    }
}

fn status_label(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::Found => "found",
        SourceStatus::Missing => "missing",
        SourceStatus::Ignored => "ignored",
        SourceStatus::Rejected => "rejected",
    }
}

fn role_label(role: &LoadReceiptRole) -> &'static str {
    match role {
        LoadReceiptRole::Library => "library",
        LoadReceiptRole::BootCodec { .. } => "boot-codec",
    }
}

fn is_no_codec_error(err: &CliError) -> bool {
    err.to_string().starts_with("no codec '")
}

fn expr_key_label(key: &Expr) -> String {
    match key {
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.as_qualified_str(),
        Expr::String(value) => value.clone(),
        Expr::Bool(value) => value.to_string(),
        Expr::Number(number) => number.canonical.clone(),
        other => format!("{other:?}"),
    }
}

fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => output.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => output.push(ch),
        }
    }
    output.push('"');
}

fn comma(output: &mut String, index: usize) {
    if index > 0 {
        output.push(',');
    }
}

fn is_json_number(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    (first.is_ascii_digit() || first == '-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E'))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
