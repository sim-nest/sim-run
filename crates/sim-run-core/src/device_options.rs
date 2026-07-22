//! Reusable route option models for device product verbs.

use std::fmt;

use sim_kernel::Symbol;

use crate::device_host::RouteArg;

const GLASSES_ROUTE_OPTIONS: &[&str] = &[
    "direct-linux",
    "android-usb",
    "neckband-local",
    "neckband-relay",
    "mobile-dock-display",
    "ble-direct",
    "web-bluetooth",
    "phone-relay",
    "controller-hid",
];

/// Closed command-line option set that resolves to open route symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceRouteOptionModel {
    options: &'static [&'static str],
}

impl DeviceRouteOptionModel {
    /// Returns the glasses route option model.
    pub const fn glasses() -> Self {
        Self {
            options: GLASSES_ROUTE_OPTIONS,
        }
    }

    /// Returns the accepted option tokens in display order.
    pub const fn options(self) -> &'static [&'static str] {
        self.options
    }

    /// Parses one option into the route argument consumed by device composition.
    pub fn parse(self, value: &str) -> Result<RouteArg, DeviceRouteOptionError> {
        if self.options.contains(&value) {
            Ok(RouteArg::new(Symbol::qualified("device/route", value)))
        } else {
            Err(DeviceRouteOptionError {
                value: value.to_owned(),
                expected: self.options,
            })
        }
    }
}

/// Error returned when a device route option is not in the selected model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRouteOptionError {
    value: String,
    expected: &'static [&'static str],
}

impl DeviceRouteOptionError {
    /// Returns the rejected option token.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the accepted option tokens.
    pub const fn expected(&self) -> &'static [&'static str] {
        self.expected
    }
}

impl fmt::Display for DeviceRouteOptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported device route '{}'; expected one of: {}",
            self.value,
            self.expected.join(", ")
        )
    }
}

impl std::error::Error for DeviceRouteOptionError {}

#[cfg(test)]
#[path = "device_options_tests.rs"]
mod tests;
