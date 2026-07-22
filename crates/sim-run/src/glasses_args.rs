use std::path::PathBuf;

use sim_kernel::{Error, Result};
use sim_run_core::{RouteArg, device_options::DeviceRouteOptionModel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassesDeviceSelection {
    Viture,
    Halo,
    Auto,
    Both,
}

impl GlassesDeviceSelection {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "viture" => Ok(Self::Viture),
            "halo" => Ok(Self::Halo),
            "auto" => Ok(Self::Auto),
            "both" => Ok(Self::Both),
            _ => Err(Error::Eval(format!("unsupported glasses device: {value}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassesDeviceKind {
    Viture,
    Halo,
    Modeled,
}

impl GlassesDeviceKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Viture => "viture",
            Self::Halo => "halo",
            Self::Modeled => "modeled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassesProfile {
    Auto,
    LumaUltra,
    Halo,
    DisplayOnly,
    Neckband,
}

impl GlassesProfile {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "luma-ultra" => Ok(Self::LumaUltra),
            "halo" => Ok(Self::Halo),
            "display-only" => Ok(Self::DisplayOnly),
            "neckband" => Ok(Self::Neckband),
            _ => Err(Error::Eval(format!("unsupported glasses profile: {value}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassesPose {
    Auto,
    None,
    Modeled,
    Viture,
    Halo,
}

impl GlassesPose {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "none" => Ok(Self::None),
            "modeled" => Ok(Self::Modeled),
            "viture" => Ok(Self::Viture),
            "halo" => Ok(Self::Halo),
            _ => Err(Error::Eval(format!("unsupported glasses pose: {value}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GlassesEncoder {
    Host,
    Neckband,
    Peer,
}

impl GlassesEncoder {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "host" => Ok(Self::Host),
            "neckband" => Ok(Self::Neckband),
            "peer" => Ok(Self::Peer),
            _ => Err(Error::Eval(format!("unsupported glasses encoder: {value}"))),
        }
    }

    pub(super) fn site(self) -> &'static str {
        match self {
            Self::Host => "host-encoder",
            Self::Neckband => "neckband-encoder",
            Self::Peer => "peer-encoder",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DisplaySelection {
    Auto,
    Index(u32),
}

impl DisplaySelection {
    fn parse(value: &str) -> Result<Self> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        let display = value
            .parse::<u32>()
            .map_err(|_| Error::Eval(format!("unsupported glasses display: {value}")))?;
        if display == 0 {
            return Err(Error::Eval("glasses display must be at least 1".to_owned()));
        }
        Ok(Self::Index(display))
    }

    pub(super) fn as_label(self) -> String {
        match self {
            Self::Auto => "auto".to_owned(),
            Self::Index(index) => index.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParsedRoute {
    pub(super) token: String,
    pub(super) route: RouteArg,
}

impl ParsedRoute {
    fn parse(value: &str) -> Result<Self> {
        let route = DeviceRouteOptionModel::glasses()
            .parse(value)
            .map_err(|error| Error::Eval(error.to_string()))?;
        Ok(Self {
            token: value.to_owned(),
            route,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GlassesArgs {
    pub(super) device: GlassesDeviceSelection,
    pub(super) profile: GlassesProfile,
    pub(super) route: Option<ParsedRoute>,
    pub(super) pose: GlassesPose,
    pub(super) mirror: bool,
    pub(super) encoder: GlassesEncoder,
    pub(super) display: DisplaySelection,
    layout: Option<PathBuf>,
    vendor_report: VendorReport,
    pub(super) dry_run: bool,
    pub(super) help: bool,
}

impl GlassesArgs {
    pub(super) fn parse(args: &[String]) -> Result<Self> {
        let mut parsed = Self {
            device: GlassesDeviceSelection::Auto,
            profile: GlassesProfile::Auto,
            route: None,
            pose: GlassesPose::Auto,
            mirror: false,
            encoder: GlassesEncoder::Host,
            display: DisplaySelection::Auto,
            layout: None,
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
                "--device" => {
                    parsed.device =
                        GlassesDeviceSelection::parse(value(args, &mut index, "--device")?)?;
                }
                "--profile" => {
                    parsed.profile = GlassesProfile::parse(value(args, &mut index, "--profile")?)?;
                }
                "--route" => {
                    parsed.route = Some(ParsedRoute::parse(value(args, &mut index, "--route")?)?);
                }
                "--pose" => {
                    parsed.pose = GlassesPose::parse(value(args, &mut index, "--pose")?)?;
                }
                "--mirror" => {
                    parsed.mirror = true;
                    index += 1;
                }
                "--encoder" => {
                    parsed.encoder = GlassesEncoder::parse(value(args, &mut index, "--encoder")?)?;
                }
                "--display" => {
                    parsed.display =
                        DisplaySelection::parse(value(args, &mut index, "--display")?)?;
                }
                "--layout" => {
                    parsed.layout = Some(PathBuf::from(value(args, &mut index, "--layout")?));
                }
                "--vendor-report" => {
                    parsed.vendor_report =
                        VendorReport::parse(value(args, &mut index, "--vendor-report")?)?;
                }
                "--dry-run" => {
                    parsed.dry_run = true;
                    index += 1;
                }
                flag if flag.starts_with("--device=") => {
                    parsed.device = GlassesDeviceSelection::parse(inline(flag, "--device=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--profile=") => {
                    parsed.profile = GlassesProfile::parse(inline(flag, "--profile=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--route=") => {
                    parsed.route = Some(ParsedRoute::parse(inline(flag, "--route=")?)?);
                    index += 1;
                }
                flag if flag.starts_with("--pose=") => {
                    parsed.pose = GlassesPose::parse(inline(flag, "--pose=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--encoder=") => {
                    parsed.encoder = GlassesEncoder::parse(inline(flag, "--encoder=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--display=") => {
                    parsed.display = DisplaySelection::parse(inline(flag, "--display=")?)?;
                    index += 1;
                }
                flag if flag.starts_with("--layout=") => {
                    parsed.layout = Some(PathBuf::from(inline(flag, "--layout=")?));
                    index += 1;
                }
                flag if flag.starts_with("--vendor-report=") => {
                    parsed.vendor_report = VendorReport::parse(inline(flag, "--vendor-report=")?)?;
                    index += 1;
                }
                other => {
                    return Err(Error::Eval(format!(
                        "unsupported glasses argument: {other}"
                    )));
                }
            }
        }
        Ok(parsed)
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

#[cfg(test)]
mod tests {
    use sim_run_core::CliCommand;

    use super::{GlassesArgs, GlassesDeviceSelection};

    #[test]
    fn parses_display_route_and_inline_options() {
        let parsed = parse_args_for_test([
            "sim",
            "glasses",
            "--device=both",
            "--route=ble-direct",
            "--display",
            "2",
            "--mirror",
            "--dry-run",
        ]);

        assert_eq!(parsed.device, GlassesDeviceSelection::Both);
        assert_eq!(parsed.route.as_ref().unwrap().token, "ble-direct");
        assert_eq!(parsed.display.as_label(), "2");
        assert!(parsed.mirror);
        assert!(parsed.dry_run);
    }

    fn parse_args_for_test<const N: usize>(args: [&str; N]) -> GlassesArgs {
        let CliCommand::Boot(boot) = sim_run_core::parse_args(args).unwrap() else {
            panic!("expected boot command");
        };
        let args = boot
            .payload
            .args
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        GlassesArgs::parse(&args).unwrap()
    }
}
