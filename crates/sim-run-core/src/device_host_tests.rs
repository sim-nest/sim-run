use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};

use crate::{
    DeviceConsentPolicy, DeviceHostSpec, DeviceHostStalePolicy, DevicePlacement, DeviceProfile,
    DeviceProviderKind, DeviceRateClass, DeviceSite, RouteArg, compose_device_host,
};

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

fn reference_placement(profile: &DeviceProfile) -> DevicePlacement {
    let codec = Symbol::qualified("codec", "surface-device");
    DevicePlacement::new(
        DeviceSite::remote(
            Symbol::qualified("device/site", "host-encoder"),
            profile.clone(),
            codec.clone(),
        ),
        DeviceSite::edge_local(
            Symbol::qualified("device/site", "edge-adapter"),
            profile.clone(),
            codec,
        ),
    )
}

#[test]
fn compose_reference_device_headless() {
    let mut cx = test_cx();
    let profile = DeviceProfile::modeled_edge();
    let placement = reference_placement(&profile);
    let spec = DeviceHostSpec::new(
        profile.clone(),
        RouteArg::headless(),
        placement.clone(),
        DeviceHostStalePolicy::HoldLast,
        DeviceConsentPolicy::Headless,
    );

    let mut session = compose_device_host(&mut cx, spec).expect("compose device host");

    assert!(session.is_live());
    assert_eq!(session.provider_kind(), DeviceProviderKind::Stub);
    assert_eq!(session.profile(), &profile);
    assert_eq!(session.device_session().profile(), &profile);
    assert_eq!(session.route(), &RouteArg::headless());
    assert_eq!(session.placement(), &placement);
    assert_eq!(
        session.adapter_loop().rate_class(),
        DeviceRateClass::Surface
    );
    assert_eq!(session.adapter_loop().interval_ms(), 50);
    assert_eq!(session.adapter_loop().sequence(), 0);
    assert_eq!(session.hub_join().route(), &RouteArg::headless());
    assert_eq!(session.hub_join().adapter_site(), &placement.adapter.symbol);

    let tick = session.adapter_loop_mut().next_tick();

    assert_eq!(tick.sequence(), 1);
    assert_eq!(tick.interval_ms(), 50);
    assert_eq!(session.adapter_loop().sequence(), 1);
}
