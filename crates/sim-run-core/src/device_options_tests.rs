use std::collections::BTreeSet;

use super::DeviceRouteOptionModel;

#[test]
fn glasses_route_option_model_accepts_every_supported_route() {
    let model = DeviceRouteOptionModel::glasses();
    assert_eq!(
        model.options(),
        [
            "direct-linux",
            "android-usb",
            "neckband-local",
            "neckband-relay",
            "mobile-dock-display",
            "ble-direct",
            "web-bluetooth",
            "phone-relay",
            "controller-hid",
        ]
    );

    let unique = model.options().iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), model.options().len());

    for value in model.options() {
        let route = model.parse(value).expect("supported glasses route");
        assert_eq!(route.symbol().namespace.as_deref(), Some("device/route"));
        assert_eq!(route.symbol().name.as_ref(), *value);
    }
}

#[test]
fn glasses_route_option_model_rejects_unknown_route() {
    let model = DeviceRouteOptionModel::glasses();
    let error = model.parse("vendor-cloud").expect_err("unknown route");

    assert_eq!(error.value(), "vendor-cloud");
    assert_eq!(error.expected(), model.options());
    assert!(error.to_string().contains("direct-linux"));
    assert!(error.to_string().contains("controller-hid"));
}
