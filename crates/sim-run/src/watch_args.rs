use std::path::PathBuf;

use sim_kernel::{Error, Result, Symbol};
use sim_run_core::{
    DeviceConsentPolicy, DeviceHostSpec, DeviceHostStalePolicy, DevicePlacement, DeviceProfile,
    DeviceSite, RouteArg,
};

const SURFACE_CODEC: &str = "surface-device";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WatchProfile {
    Glance,
    GlanceLarge,
    Sport,
    Sleep,
}

impl WatchProfile {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "watch-glance" => Ok(Self::Glance),
            "watch-glance-large" => Ok(Self::GlanceLarge),
            "watch-sport" => Ok(Self::Sport),
            "watch-sleep" => Ok(Self::Sleep),
            _ => Err(Error::Eval(format!("unsupported watch profile: {value}"))),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Glance => "watch-glance",
            Self::GlanceLarge => "watch-glance-large",
            Self::Sport => "watch-sport",
            Self::Sleep => "watch-sleep",
        }
    }

    pub(super) fn tier(self) -> &'static str {
        match self {
            Self::Sleep => "sensor",
            _ => "sensor+actuator",
        }
    }

    pub(super) fn rate_label(self) -> &'static str {
        match self {
            Self::Sport => "1/4hz",
            _ => "1/1hz",
        }
    }

    fn profile(self) -> DeviceProfile {
        let mut streams = vec![
            Symbol::qualified("device/stream", "battery"),
            Symbol::qualified("device/stream", "motion"),
            Symbol::qualified("device/stream", "heart-rate"),
        ];
        if matches!(self, Self::Sport) {
            streams.push(Symbol::qualified("device/stream", "location"));
        }
        let inputs = match self {
            Self::Sleep => Vec::new(),
            _ => vec![
                Symbol::qualified("device/input", "button"),
                Symbol::qualified("device/input", "tap"),
                Symbol::qualified("device/input", "raise"),
            ],
        };
        let outputs = match self {
            Self::Sleep => Vec::new(),
            _ => vec![
                Symbol::qualified("device/output", "screen"),
                Symbol::qualified("device/output", "haptic"),
                Symbol::qualified("device/output", "notification"),
            ],
        };
        DeviceProfile::new(
            Symbol::qualified("device", self.as_str()),
            streams,
            inputs,
            outputs,
            vec![
                Symbol::qualified("device/sample", "worn-event"),
                Symbol::qualified("device/sample", "caps"),
            ],
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WatchRoute {
    Modeled,
    Import,
    Ble,
    Relay,
    ZeppBridge,
    WifiLocal,
}

impl WatchRoute {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "modeled" => Ok(Self::Modeled),
            "import" => Ok(Self::Import),
            "ble" => Ok(Self::Ble),
            "relay" => Ok(Self::Relay),
            "zepp-bridge" => Ok(Self::ZeppBridge),
            "wifi-local" => Ok(Self::WifiLocal),
            _ => Err(Error::Eval(format!("unsupported watch route: {value}"))),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Modeled => "modeled",
            Self::Import => "import",
            Self::Ble => "ble",
            Self::Relay => "relay",
            Self::ZeppBridge => "zepp-bridge",
            Self::WifiLocal => "wifi-local",
        }
    }

    fn route_arg(self) -> RouteArg {
        RouteArg::new(Symbol::qualified("device/route", self.as_str()))
    }

    fn requires_live_provider(self) -> bool {
        matches!(
            self,
            Self::Ble | Self::Relay | Self::ZeppBridge | Self::WifiLocal
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WatchSource {
    Modeled,
    Import(PathBuf),
    Live,
}

impl WatchSource {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Modeled => "modeled",
            Self::Import(_) => "import",
            Self::Live => "live",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WatchFleet {
    One,
    Pair,
}

impl WatchFleet {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "one" => Ok(Self::One),
            "pair" => Ok(Self::Pair),
            _ => Err(Error::Eval(format!("unsupported watch fleet: {value}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VendorReport {
    Never,
    Ask,
    Allow,
}

impl VendorReport {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "never" => Ok(Self::Never),
            "ask" => Ok(Self::Ask),
            "allow" => Ok(Self::Allow),
            _ => Err(Error::Eval(format!(
                "unsupported vendor report mode: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WatchArgs {
    pub(super) profile: WatchProfile,
    route: Option<WatchRoute>,
    pub(super) source: WatchSource,
    fleet: WatchFleet,
    consent: Option<PathBuf>,
    asr_site: Option<Symbol>,
    vendor_report: VendorReport,
    pub(super) dry_run: bool,
    pub(super) help: bool,
}

impl WatchArgs {
    pub(super) fn parse(args: &[String]) -> Result<Self> {
        let mut parsed = Self {
            profile: WatchProfile::Glance,
            route: None,
            source: WatchSource::Modeled,
            fleet: WatchFleet::One,
            consent: None,
            asr_site: None,
            vendor_report: VendorReport::Never,
            dry_run: false,
            help: false,
        };
        let mut index = 1;
        while index < args.len() {
            match args[index].as_str() {
                "--help" | "-h" => {
                    parsed.help = true;
                    index += 1;
                }
                "--profile" => {
                    parsed.profile = WatchProfile::parse(value(args, &mut index, "--profile")?)?;
                }
                "--route" => {
                    parsed.route = Some(WatchRoute::parse(value(args, &mut index, "--route")?)?);
                }
                "--source" => {
                    parsed.source = parse_source(args, &mut index)?;
                }
                "--fleet" => {
                    parsed.fleet = WatchFleet::parse(value(args, &mut index, "--fleet")?)?;
                }
                "--consent" => {
                    parsed.consent = Some(PathBuf::from(value(args, &mut index, "--consent")?));
                }
                "--asr-site" => {
                    parsed.asr_site = Some(Symbol::new(value(args, &mut index, "--asr-site")?));
                }
                "--vendor-report" => {
                    parsed.vendor_report =
                        VendorReport::parse(value(args, &mut index, "--vendor-report")?)?;
                }
                "--dry-run" => {
                    parsed.dry_run = true;
                    index += 1;
                }
                flag if flag.starts_with("--profile=") => {
                    parsed.profile = WatchProfile::parse(inline(flag, "--profile=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--route=") => {
                    parsed.route = Some(WatchRoute::parse(inline(flag, "--route=")?)?);
                    index += 1;
                }
                flag if flag.starts_with("--source=") => {
                    parsed.source =
                        parse_inline_source(inline(flag, "--source=")?, args, &mut index)?;
                }
                flag if flag.starts_with("--fleet=") => {
                    parsed.fleet = WatchFleet::parse(inline(flag, "--fleet=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--consent=") => {
                    parsed.consent = Some(PathBuf::from(inline(flag, "--consent=")?));
                    index += 1;
                }
                flag if flag.starts_with("--asr-site=") => {
                    parsed.asr_site = Some(Symbol::new(inline(flag, "--asr-site=")?));
                    index += 1;
                }
                flag if flag.starts_with("--vendor-report=") => {
                    parsed.vendor_report = VendorReport::parse(inline(flag, "--vendor-report=")?)?;
                    index += 1;
                }
                other => return Err(Error::Eval(format!("unsupported watch argument: {other}"))),
            }
        }
        Ok(parsed)
    }

    pub(super) fn plan(&self) -> Result<WatchPlan> {
        let route = self.route.unwrap_or(match self.source {
            WatchSource::Modeled => WatchRoute::Modeled,
            WatchSource::Import(_) => WatchRoute::Import,
            WatchSource::Live => WatchRoute::Ble,
        });
        let profile = self.profile.profile();
        let spec = DeviceHostSpec::new(
            profile.clone(),
            route.route_arg(),
            watch_placement(&profile),
            DeviceHostStalePolicy::HoldLast,
            consent_policy(self),
        );
        Ok(WatchPlan {
            route,
            spec,
            glance_adapter: "configured(device)",
            unavailable: unavailable_reasons(self, route),
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct WatchPlan {
    pub(super) route: WatchRoute,
    pub(super) spec: DeviceHostSpec,
    pub(super) glance_adapter: &'static str,
    pub(super) unavailable: Vec<String>,
}

fn parse_source(args: &[String], index: &mut usize) -> Result<WatchSource> {
    let selected = value(args, index, "--source")?;
    parse_source_value(selected, args, index)
}

fn parse_inline_source(value: &str, args: &[String], index: &mut usize) -> Result<WatchSource> {
    *index += 1;
    parse_source_value(value, args, index)
}

fn parse_source_value(value: &str, args: &[String], index: &mut usize) -> Result<WatchSource> {
    match value {
        "modeled" => Ok(WatchSource::Modeled),
        "live" => Ok(WatchSource::Live),
        "import" => {
            let Some(path) = args.get(*index) else {
                return Err(Error::Eval(
                    "--source import requires a file path".to_owned(),
                ));
            };
            *index += 1;
            Ok(WatchSource::Import(PathBuf::from(path)))
        }
        _ => Err(Error::Eval(format!("unsupported watch source: {value}"))),
    }
}

fn value<'a>(args: &'a [String], index: &mut usize, flag: &str) -> Result<&'a str> {
    let Some(value) = args.get(*index + 1) else {
        return Err(Error::Eval(format!("{flag} requires a value")));
    };
    *index += 2;
    Ok(value)
}

fn inline<'a>(arg: &'a str, prefix: &str) -> Result<&'a str> {
    let value = &arg[prefix.len()..];
    if value.is_empty() {
        Err(Error::Eval(format!(
            "{} requires a value",
            prefix.trim_end_matches('=')
        )))
    } else {
        Ok(value)
    }
}

fn watch_placement(profile: &DeviceProfile) -> DevicePlacement {
    let codec = Symbol::qualified("codec", SURFACE_CODEC);
    DevicePlacement::new(
        DeviceSite::remote(
            Symbol::qualified("device/site", "watch-encoder"),
            profile.clone(),
            codec.clone(),
        ),
        DeviceSite::edge_local(
            Symbol::qualified("device/site", "watch-adapter"),
            profile.clone(),
            codec,
        ),
    )
}

fn consent_policy(args: &WatchArgs) -> DeviceConsentPolicy {
    if matches!(args.source, WatchSource::Modeled | WatchSource::Import(_)) {
        return DeviceConsentPolicy::Headless;
    }
    DeviceConsentPolicy::RequireReceipt {
        subject: Symbol::qualified("watch/consent", "receipt"),
    }
}

fn unavailable_reasons(args: &WatchArgs, route: WatchRoute) -> Vec<String> {
    let mut reasons = Vec::new();
    if route.requires_live_provider() {
        reasons.push(format!(
            "route={} reason=live provider is not installed in this build",
            route.as_str()
        ));
    }
    if matches!(args.source, WatchSource::Live) && args.consent.is_none() {
        reasons.push("source=live reason=--consent is required".to_owned());
    }
    if matches!(args.vendor_report, VendorReport::Allow) && args.consent.is_none() {
        reasons.push("vendor-report=allow reason=--consent is required".to_owned());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use sim_run_core::{CliCommand, parse_args};

    use super::{WatchArgs, WatchRoute, WatchSource};

    #[test]
    fn source_import_defaults_route_to_import() {
        let CliCommand::Boot(boot) =
            parse_args(["sim", "watch", "--source", "import", "fixture.json"]).unwrap()
        else {
            panic!("expected boot");
        };
        let args = boot
            .payload
            .args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let parsed = WatchArgs::parse(&args).unwrap();
        let plan = parsed.plan().unwrap();

        assert!(matches!(parsed.source, WatchSource::Import(_)));
        assert_eq!(plan.route, WatchRoute::Import);
    }
}
