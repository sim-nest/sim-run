use sim_kernel::{Error, Result, Symbol};
use sim_run_core::{
    DeviceConsentPolicy, DeviceHostSpec, DeviceHostStalePolicy, DevicePlacement, DeviceProfile,
    DeviceSite, RouteArg,
};

use crate::glasses_args::{
    GlassesArgs, GlassesDeviceKind, GlassesDeviceSelection, GlassesEncoder, GlassesPose,
    GlassesProfile,
};

const SURFACE_CODEC: &str = "surface-device";
const VITURE_CARINA_LANE: &str = "viture_carina";
const VITURE_LEGACY_IMU_LANE: &str = "viture_legacy";
const VITURE_UVC_CAMERA_LANE: &str = "viture_uvc_cam";
const HALO_BLE_DIRECT_LANE: &str = "halo_ble_direct";
const HALO_WEB_BLUETOOTH_LANE: &str = "halo_web_bt";
const HALO_PHONE_RELAY_LANE: &str = "halo_phone_relay";

impl GlassesArgs {
    pub(super) fn plan(&self) -> Result<GlassesPlan> {
        let kinds = self.device_kinds();
        let co_use = matches!(self.device, GlassesDeviceSelection::Both);
        let devices = kinds
            .into_iter()
            .map(|kind| self.device_plan(kind))
            .collect::<Result<Vec<_>>>()?;
        Ok(GlassesPlan { devices, co_use })
    }

    fn device_kinds(&self) -> Vec<GlassesDeviceKind> {
        match self.device {
            GlassesDeviceSelection::Viture => vec![GlassesDeviceKind::Viture],
            GlassesDeviceSelection::Halo => vec![GlassesDeviceKind::Halo],
            GlassesDeviceSelection::Both => {
                vec![GlassesDeviceKind::Viture, GlassesDeviceKind::Halo]
            }
            GlassesDeviceSelection::Auto => vec![self.auto_device_kind()],
        }
    }

    fn auto_device_kind(&self) -> GlassesDeviceKind {
        if self.mirror || matches!(self.profile, GlassesProfile::DisplayOnly) {
            return GlassesDeviceKind::Modeled;
        }
        match self.profile {
            GlassesProfile::LumaUltra | GlassesProfile::Neckband => GlassesDeviceKind::Viture,
            GlassesProfile::Halo => GlassesDeviceKind::Halo,
            GlassesProfile::Auto | GlassesProfile::DisplayOnly => self
                .route
                .as_ref()
                .and_then(|route| device_for_route(&route.token))
                .unwrap_or(GlassesDeviceKind::Modeled),
        }
    }

    fn device_plan(&self, kind: GlassesDeviceKind) -> Result<GlassesDevicePlan> {
        let _display_label = self.display.as_label();
        let profile = ResolvedProfile::for_device(kind, self.profile, self.mirror)?;
        let hardware_route = self.route.as_ref().and_then(|route| {
            lane_for_route(kind, &route.token, self.pose).map(|lane| (route, lane))
        });
        let fallback = hardware_route.and_then(|(route, lane)| {
            (!bringup_verified(lane)).then(|| GlassesFallback {
                route: route.token.clone(),
                reason: format!("lane {lane} not verified"),
            })
        });
        let route = match (&self.route, &fallback) {
            (_, Some(_)) | (None, _) => RouteArg::headless(),
            (Some(route), None) => route.route.clone(),
        };
        let consent = if route == RouteArg::headless() || self.mirror {
            DeviceConsentPolicy::Headless
        } else {
            DeviceConsentPolicy::RequireReceipt {
                subject: Symbol::qualified("glasses/consent", "receipt"),
            }
        };
        let spec = DeviceHostSpec::new(
            profile.profile.clone(),
            route,
            glasses_placement(&profile, self.encoder),
            profile.stale,
            consent,
        );
        Ok(GlassesDevicePlan {
            label: kind.label(),
            profile_label: profile.profile_label,
            tier_label: profile.tier_label,
            adapter_label: profile.adapter_label,
            loop_label: profile.loop_label,
            spec,
            fallback,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct GlassesPlan {
    pub(super) devices: Vec<GlassesDevicePlan>,
    pub(super) co_use: bool,
}

#[derive(Clone, Debug)]
pub(super) struct GlassesDevicePlan {
    pub(super) label: &'static str,
    pub(super) profile_label: &'static str,
    pub(super) tier_label: &'static str,
    pub(super) adapter_label: &'static str,
    pub(super) loop_label: &'static str,
    pub(super) spec: DeviceHostSpec,
    pub(super) fallback: Option<GlassesFallback>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GlassesFallback {
    pub(super) route: String,
    pub(super) reason: String,
}

#[derive(Clone, Debug)]
struct ResolvedProfile {
    profile: DeviceProfile,
    profile_label: &'static str,
    tier_label: &'static str,
    adapter_label: &'static str,
    loop_label: &'static str,
    stale: DeviceHostStalePolicy,
}

impl ResolvedProfile {
    fn for_device(kind: GlassesDeviceKind, selected: GlassesProfile, mirror: bool) -> Result<Self> {
        if mirror {
            return Ok(display_only_profile(kind));
        }
        match kind {
            GlassesDeviceKind::Viture => match selected {
                GlassesProfile::Auto | GlassesProfile::LumaUltra => Ok(viture_profile()),
                GlassesProfile::DisplayOnly => Ok(display_only_profile(kind)),
                GlassesProfile::Neckband => Ok(neckband_profile()),
                GlassesProfile::Halo => Err(Error::Eval(
                    "profile halo is not compatible with device viture".to_owned(),
                )),
            },
            GlassesDeviceKind::Halo => match selected {
                GlassesProfile::Auto | GlassesProfile::Halo => Ok(halo_profile()),
                GlassesProfile::DisplayOnly => Ok(display_only_profile(kind)),
                GlassesProfile::LumaUltra | GlassesProfile::Neckband => Err(Error::Eval(format!(
                    "profile {} is not compatible with device halo",
                    profile_label(selected)
                ))),
            },
            GlassesDeviceKind::Modeled => Ok(display_only_profile(kind)),
        }
    }
}

fn viture_profile() -> ResolvedProfile {
    ResolvedProfile {
        profile: DeviceProfile::new(
            Symbol::qualified("device", "viture-luma-ultra"),
            vec![
                Symbol::qualified("device/stream", "xr-pose"),
                Symbol::qualified("device/stream", "xr-camera"),
            ],
            vec![
                Symbol::qualified("device/input", "gaze"),
                Symbol::qualified("device/input", "controller"),
            ],
            vec![
                Symbol::qualified("device/output", "left-display"),
                Symbol::qualified("device/output", "right-display"),
            ],
            vec![
                Symbol::qualified("device/sample", "xr-pose"),
                Symbol::qualified("device/sample", "xr-camera-frame"),
            ],
        ),
        profile_label: "luma-ultra",
        tier_label: "rich",
        adapter_label: "reprojector",
        loop_label: "60/120",
        stale: DeviceHostStalePolicy::PredictClamp,
    }
}

fn halo_profile() -> ResolvedProfile {
    ResolvedProfile {
        profile: DeviceProfile::new(
            Symbol::qualified("device", "halo"),
            vec![Symbol::qualified("device/stream", "halo-motion")],
            vec![Symbol::qualified("device/input", "tap")],
            vec![Symbol::qualified("device/output", "hud")],
            vec![
                Symbol::qualified("device/sample", "lua-diff-frame"),
                Symbol::qualified("device/sample", "xr-camera-frame"),
            ],
        ),
        profile_label: "halo",
        tier_label: "actuator",
        adapter_label: "glance",
        loop_label: "5/30",
        stale: DeviceHostStalePolicy::HoldLast,
    }
}

fn neckband_profile() -> ResolvedProfile {
    let mut profile = viture_profile();
    profile.profile.device = Symbol::qualified("device", "viture-neckband");
    profile.profile_label = "neckband";
    profile
}

fn display_only_profile(kind: GlassesDeviceKind) -> ResolvedProfile {
    ResolvedProfile {
        profile: DeviceProfile::new(
            Symbol::qualified("device", kind.label()),
            Vec::new(),
            Vec::new(),
            vec![Symbol::qualified("device/output", "mirror-display")],
            vec![Symbol::qualified("device/sample", "surface-frame")],
        ),
        profile_label: "display-only",
        tier_label: "display",
        adapter_label: "mirror",
        loop_label: "1/1",
        stale: DeviceHostStalePolicy::HoldLast,
    }
}

fn glasses_placement(profile: &ResolvedProfile, encoder: GlassesEncoder) -> DevicePlacement {
    let codec = Symbol::qualified("codec", SURFACE_CODEC);
    DevicePlacement::new(
        DeviceSite::host_local(
            Symbol::qualified("device/site", encoder.site()),
            profile.profile.clone(),
            codec.clone(),
        ),
        DeviceSite::edge_local(
            Symbol::qualified("device/site", profile.adapter_label),
            profile.profile.clone(),
            codec,
        ),
    )
}

fn device_for_route(route: &str) -> Option<GlassesDeviceKind> {
    match route {
        "direct-linux"
        | "android-usb"
        | "neckband-local"
        | "neckband-relay"
        | "mobile-dock-display" => Some(GlassesDeviceKind::Viture),
        "ble-direct" | "web-bluetooth" | "phone-relay" | "controller-hid" => {
            Some(GlassesDeviceKind::Halo)
        }
        _ => None,
    }
}

fn lane_for_route(kind: GlassesDeviceKind, route: &str, pose: GlassesPose) -> Option<&'static str> {
    match (kind, route) {
        (GlassesDeviceKind::Viture, "direct-linux" | "android-usb") => {
            if matches!(pose, GlassesPose::None) {
                Some(VITURE_LEGACY_IMU_LANE)
            } else {
                Some(VITURE_CARINA_LANE)
            }
        }
        (GlassesDeviceKind::Viture, "neckband-local" | "neckband-relay") => {
            Some(VITURE_LEGACY_IMU_LANE)
        }
        (GlassesDeviceKind::Viture, "mobile-dock-display") => Some(VITURE_UVC_CAMERA_LANE),
        (GlassesDeviceKind::Halo, "ble-direct" | "controller-hid") => Some(HALO_BLE_DIRECT_LANE),
        (GlassesDeviceKind::Halo, "web-bluetooth") => Some(HALO_WEB_BLUETOOTH_LANE),
        (GlassesDeviceKind::Halo, "phone-relay") => Some(HALO_PHONE_RELAY_LANE),
        _ => None,
    }
}

fn bringup_verified(_lane: &str) -> bool {
    false
}

fn profile_label(profile: GlassesProfile) -> &'static str {
    match profile {
        GlassesProfile::Auto => "auto",
        GlassesProfile::LumaUltra => "luma-ultra",
        GlassesProfile::Halo => "halo",
        GlassesProfile::DisplayOnly => "display-only",
        GlassesProfile::Neckband => "neckband",
    }
}

#[cfg(test)]
mod tests {
    use sim_run_core::CliCommand;

    use crate::glasses_args::GlassesArgs;

    #[test]
    fn auto_without_hardware_names_modeled_display_only() {
        let parsed = parse_args_for_test(["sim", "glasses", "--device", "auto", "--dry-run"]);
        let plan = parsed.plan().unwrap();

        assert_eq!(plan.devices.len(), 1);
        assert_eq!(plan.devices[0].label, "modeled");
        assert_eq!(plan.devices[0].profile_label, "display-only");
        assert!(plan.devices[0].fallback.is_none());
    }

    #[test]
    fn unverified_route_falls_back_to_headless() {
        let parsed = parse_args_for_test([
            "sim",
            "glasses",
            "--device",
            "halo",
            "--route",
            "ble-direct",
        ]);
        let plan = parsed.plan().unwrap();

        assert_eq!(plan.devices.len(), 1);
        assert_eq!(
            plan.devices[0].fallback.as_ref().unwrap().reason,
            "lane halo_ble_direct not verified"
        );
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
