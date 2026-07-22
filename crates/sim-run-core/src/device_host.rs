use std::fmt;

use sim_kernel::{Cx, Error as KernelError, Result, Symbol};

/// Stream-facing profile advertised by a composed device route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Stable device identity.
    pub device: Symbol,
    /// Sample streams this device can emit.
    pub streams: Vec<Symbol>,
    /// Input controls accepted by the device.
    pub inputs: Vec<Symbol>,
    /// Output actuators exposed by the device.
    pub outputs: Vec<Symbol>,
    /// Sample kinds the provider may return.
    pub sample_kinds: Vec<Symbol>,
}

impl DeviceProfile {
    /// Builds a device profile from stable route metadata.
    pub fn new(
        device: Symbol,
        streams: Vec<Symbol>,
        inputs: Vec<Symbol>,
        outputs: Vec<Symbol>,
        sample_kinds: Vec<Symbol>,
    ) -> Self {
        Self {
            device,
            streams,
            inputs,
            outputs,
            sample_kinds,
        }
    }

    /// Builds the deterministic modeled edge profile used by tests and docs.
    pub fn modeled_edge() -> Self {
        Self::new(
            Symbol::qualified("device", "modeled-edge"),
            vec![
                Symbol::qualified("device/stream", "battery"),
                Symbol::qualified("device/stream", "motion"),
            ],
            vec![Symbol::qualified("device/input", "button")],
            vec![
                Symbol::qualified("device/output", "screen"),
                Symbol::qualified("device/output", "haptic"),
            ],
            vec![Symbol::qualified("device/sample", "caps")],
        )
    }

    /// Returns whether this profile advertises `sample_kind`.
    pub fn supports_sample_kind(&self, sample_kind: &Symbol) -> bool {
        self.sample_kinds.contains(sample_kind)
    }
}

/// Placement locality advertised by a device site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSiteLocality {
    /// Device adapter runs at the device or edge boundary.
    EdgeLocal,
    /// Site runs on the host but not at the device edge.
    HostLocal,
    /// Site crosses a remote transport boundary.
    Remote,
}

/// Export-record-style descriptor for a device route site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSite {
    /// Stable site symbol exported by the route provider.
    pub symbol: Symbol,
    /// Device profile carried by this site export.
    pub profile: DeviceProfile,
    /// Surface codec for device samples and commands.
    pub surface_codec_id: Symbol,
    /// Locality used by placement validation.
    pub locality: DeviceSiteLocality,
}

impl DeviceSite {
    /// Builds a device site descriptor.
    pub fn new(
        symbol: Symbol,
        profile: DeviceProfile,
        surface_codec_id: Symbol,
        locality: DeviceSiteLocality,
    ) -> Self {
        Self {
            symbol,
            profile,
            surface_codec_id,
            locality,
        }
    }

    /// Builds a device or edge-local site descriptor.
    pub fn edge_local(symbol: Symbol, profile: DeviceProfile, surface_codec_id: Symbol) -> Self {
        Self::new(
            symbol,
            profile,
            surface_codec_id,
            DeviceSiteLocality::EdgeLocal,
        )
    }

    /// Builds a host-local site descriptor.
    pub fn host_local(symbol: Symbol, profile: DeviceProfile, surface_codec_id: Symbol) -> Self {
        Self::new(
            symbol,
            profile,
            surface_codec_id,
            DeviceSiteLocality::HostLocal,
        )
    }

    /// Builds a remote site descriptor.
    pub fn remote(symbol: Symbol, profile: DeviceProfile, surface_codec_id: Symbol) -> Self {
        Self::new(
            symbol,
            profile,
            surface_codec_id,
            DeviceSiteLocality::Remote,
        )
    }

    /// Returns whether this site is local enough for a latency-critical adapter.
    pub fn is_edge_local(&self) -> bool {
        self.locality == DeviceSiteLocality::EdgeLocal
    }
}

/// Placement plan for a device surface encoder and adapter pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevicePlacement {
    /// Site that encodes samples and commands for the selected surface codec.
    pub encoder: DeviceSite,
    /// Latency-critical adapter site placed at the device edge.
    pub adapter: DeviceSite,
}

impl DevicePlacement {
    /// Builds a device placement plan from an encoder and adapter site.
    pub fn new(encoder: DeviceSite, adapter: DeviceSite) -> Self {
        Self { encoder, adapter }
    }

    /// Validates placement invariants for live device operation.
    pub fn validate(&self) -> std::result::Result<(), DevicePlacementError> {
        if self.adapter.is_edge_local() {
            Ok(())
        } else {
            Err(DevicePlacementError::AdapterMustBeEdgeLocal)
        }
    }
}

/// Device placement validation error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePlacementError {
    /// The latency-critical adapter is not device or edge local.
    AdapterMustBeEdgeLocal,
}

impl fmt::Display for DevicePlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdapterMustBeEdgeLocal => f.write_str("device adapter must be edge-local"),
        }
    }
}

impl std::error::Error for DevicePlacementError {}

/// Provider that opens one composed device session.
pub trait DeviceProvider: Send {
    /// Opens a provider-owned device session.
    fn open(&self) -> Result<Box<dyn DeviceSession>>;
}

/// Open device session used by a composed route.
pub trait DeviceSession: Send {
    /// Returns the profile for this session.
    fn profile(&self) -> &DeviceProfile;

    /// Starts sample or command processing.
    fn start(&mut self) -> Result<()>;

    /// Stops sample or command processing and releases session resources.
    fn stop(&mut self) -> Result<()>;
}

/// Hardware-free provider used when no concrete device provider is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StubProvider {
    profile: DeviceProfile,
}

impl StubProvider {
    /// Builds a stub provider for the supplied profile.
    pub fn new(profile: DeviceProfile) -> Self {
        Self { profile }
    }

    /// Returns the profile this stub advertises for browse and placement.
    pub fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    /// Builds an unopened stub session for provider-surface validation.
    pub fn session(&self) -> StubSession {
        StubSession::new(self.profile.clone())
    }
}

impl DeviceProvider for StubProvider {
    fn open(&self) -> Result<Box<dyn DeviceSession>> {
        Ok(Box::new(self.session()))
    }
}

/// Hardware-free session used by headless device composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StubSession {
    profile: DeviceProfile,
    started: bool,
}

impl StubSession {
    /// Builds a stub session for the supplied profile.
    pub fn new(profile: DeviceProfile) -> Self {
        Self {
            profile,
            started: false,
        }
    }

    /// Returns whether this stub session has been started.
    pub fn is_started(&self) -> bool {
        self.started
    }
}

impl DeviceSession for StubSession {
    fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    fn start(&mut self) -> Result<()> {
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.started = false;
        Ok(())
    }
}

/// Route selected by a product verb before device host composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteArg {
    symbol: Symbol,
}

impl RouteArg {
    /// Builds a route from a stable route symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self { symbol }
    }

    /// Builds the hardware-free route used by headless device composition.
    pub fn headless() -> Self {
        Self::new(Symbol::qualified("device/route", "headless"))
    }

    /// Returns the stable route symbol.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
}

/// Policy applied when a device sample or rendered frame becomes stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceHostStalePolicy {
    /// Keep the last accepted sample or rendered frame visible.
    HoldLast,
    /// Predict briefly from the last accepted sample, then clamp to the last frame.
    PredictClamp,
    /// Replace stale output with a blank sample or surface frame.
    Blank,
    /// Refuse stale output until the provider yields a fresh sample.
    Refuse,
}

/// Consent policy carried by device verbs into host composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceConsentPolicy {
    /// The route is explicitly headless and requires no interactive receipt.
    Headless,
    /// A human-visible receipt must be associated with the route before use.
    RequireReceipt {
        /// Stable subject that owns the receipt.
        subject: Symbol,
    },
}

/// Coarse rate class for pacing the device adapter loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRateClass {
    /// Low-rate sensor stream with no local controls or surface output.
    Sparse,
    /// Interactive device with local controls but no surface output.
    Interactive,
    /// Surface-capable device with local output that benefits from tight pacing.
    Surface,
}

impl DeviceRateClass {
    fn interval_ms(self) -> u64 {
        match self {
            Self::Sparse => 1_000,
            Self::Interactive => 100,
            Self::Surface => 50,
        }
    }
}

/// Derives the adapter-loop rate class from stream-facing device metadata.
pub fn derive_device_rate_class(profile: &DeviceProfile) -> DeviceRateClass {
    if !profile.outputs.is_empty() {
        DeviceRateClass::Surface
    } else if !profile.inputs.is_empty() {
        DeviceRateClass::Interactive
    } else {
        DeviceRateClass::Sparse
    }
}

/// Input required to compose a device host session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceHostSpec {
    /// Stream-facing device profile selected by the product verb.
    pub profile: DeviceProfile,
    /// Route selected by the product verb.
    pub route: RouteArg,
    /// Placement plan for the surface encoder and edge-local adapter.
    pub placement: DevicePlacement,
    /// Stale-sample policy used by the adapter loop.
    pub stale: DeviceHostStalePolicy,
    /// Consent policy required before the device route is used.
    pub consent: DeviceConsentPolicy,
}

impl DeviceHostSpec {
    /// Builds a device host composition request.
    pub fn new(
        profile: DeviceProfile,
        route: RouteArg,
        placement: DevicePlacement,
        stale: DeviceHostStalePolicy,
        consent: DeviceConsentPolicy,
    ) -> Self {
        Self {
            profile,
            route,
            placement,
            stale,
            consent,
        }
    }
}

/// Provider source selected during device host composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceProviderKind {
    /// A concrete provider instance supplied the session.
    Instance,
    /// No concrete provider was supplied, so the helper composed a stub session.
    Stub,
}

/// One scheduled adapter-loop tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterTick {
    sequence: u64,
    interval_ms: u64,
}

impl AdapterTick {
    /// Returns the monotonic tick sequence.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the delay before this tick in milliseconds.
    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// Pacing plan for a composed device adapter loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAdapterLoopPlan {
    rate_class: DeviceRateClass,
    stale: DeviceHostStalePolicy,
    route: RouteArg,
    device: Symbol,
    sequence: u64,
}

impl DeviceAdapterLoopPlan {
    /// Builds a pacing plan from the selected profile, route, and stale policy.
    pub fn for_profile(
        profile: &DeviceProfile,
        route: RouteArg,
        stale: DeviceHostStalePolicy,
    ) -> Self {
        Self {
            rate_class: derive_device_rate_class(profile),
            stale,
            route,
            device: profile.device.clone(),
            sequence: 0,
        }
    }

    /// Returns the rate class used by this adapter loop.
    pub fn rate_class(&self) -> DeviceRateClass {
        self.rate_class
    }

    /// Returns the stale-sample policy used by this adapter loop.
    pub fn stale_policy(&self) -> DeviceHostStalePolicy {
        self.stale
    }

    /// Returns the route paced by this adapter loop.
    pub fn route(&self) -> &RouteArg {
        &self.route
    }

    /// Returns the selected device symbol.
    pub fn device(&self) -> &Symbol {
        &self.device
    }

    /// Returns the current tick sequence without advancing it.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the delay between adapter ticks in milliseconds.
    pub fn interval_ms(&self) -> u64 {
        self.rate_class.interval_ms()
    }

    /// Advances the adapter clock by one paced tick.
    pub fn next_tick(&mut self) -> AdapterTick {
        self.sequence += 1;
        AdapterTick {
            sequence: self.sequence,
            interval_ms: self.interval_ms(),
        }
    }
}

/// Surface-hub join plan produced by device host composition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSurfaceHubJoin {
    route: RouteArg,
    device: Symbol,
    adapter_site: Symbol,
}

impl DeviceSurfaceHubJoin {
    /// Builds a surface-hub join plan.
    pub fn new(route: RouteArg, device: Symbol, adapter_site: Symbol) -> Self {
        Self {
            route,
            device,
            adapter_site,
        }
    }

    /// Returns the route associated with the hub join.
    pub fn route(&self) -> &RouteArg {
        &self.route
    }

    /// Returns the device associated with the hub join.
    pub fn device(&self) -> &Symbol {
        &self.device
    }

    /// Returns the edge-local adapter site joined to the hub.
    pub fn adapter_site(&self) -> &Symbol {
        &self.adapter_site
    }
}

/// Composed device host session returned to per-device verbs.
pub struct DeviceEdgeSession {
    spec: DeviceHostSpec,
    provider_kind: DeviceProviderKind,
    session: Box<dyn DeviceSession>,
    adapter_loop: DeviceAdapterLoopPlan,
    hub_join: DeviceSurfaceHubJoin,
    live: bool,
}

impl DeviceEdgeSession {
    /// Returns whether host composition joined a device session.
    pub fn is_live(&self) -> bool {
        self.live
    }

    /// Returns the provider source used for this session.
    pub fn provider_kind(&self) -> DeviceProviderKind {
        self.provider_kind
    }

    /// Returns the selected stream-facing profile.
    pub fn profile(&self) -> &DeviceProfile {
        &self.spec.profile
    }

    /// Returns the selected route.
    pub fn route(&self) -> &RouteArg {
        &self.spec.route
    }

    /// Returns the validated device placement.
    pub fn placement(&self) -> &DevicePlacement {
        &self.spec.placement
    }

    /// Returns the selected stale-sample policy.
    pub fn stale_policy(&self) -> DeviceHostStalePolicy {
        self.spec.stale
    }

    /// Returns the selected consent policy.
    pub fn consent_policy(&self) -> &DeviceConsentPolicy {
        &self.spec.consent
    }

    /// Returns the immutable adapter-loop pacing plan.
    pub fn adapter_loop(&self) -> &DeviceAdapterLoopPlan {
        &self.adapter_loop
    }

    /// Returns the mutable adapter-loop pacing plan.
    pub fn adapter_loop_mut(&mut self) -> &mut DeviceAdapterLoopPlan {
        &mut self.adapter_loop
    }

    /// Returns the surface-hub join plan.
    pub fn hub_join(&self) -> &DeviceSurfaceHubJoin {
        &self.hub_join
    }

    /// Returns the composed device session.
    pub fn device_session(&self) -> &dyn DeviceSession {
        self.session.as_ref()
    }

    /// Returns the composed device session mutably.
    pub fn device_session_mut(&mut self) -> &mut dyn DeviceSession {
        self.session.as_mut()
    }
}

/// Installs the base device boot requirements into an existing context.
pub fn install_device_bases(cx: &mut Cx) -> Result<()> {
    cx.factory().nil().map(|_| ())
}

/// Composes a device host session using a hardware-free stub provider.
pub fn compose_device_host(cx: &mut Cx, spec: DeviceHostSpec) -> Result<DeviceEdgeSession> {
    let provider = StubProvider::new(spec.profile.clone());
    join_device_session(cx, spec, DeviceProviderKind::Stub, provider.open()?)
}

/// Composes a device host session using a supplied provider instance.
pub fn compose_device_host_with_provider<P>(
    cx: &mut Cx,
    spec: DeviceHostSpec,
    provider: &P,
) -> Result<DeviceEdgeSession>
where
    P: DeviceProvider + ?Sized,
{
    join_device_session(cx, spec, DeviceProviderKind::Instance, provider.open()?)
}

fn join_device_session(
    cx: &mut Cx,
    spec: DeviceHostSpec,
    provider_kind: DeviceProviderKind,
    session: Box<dyn DeviceSession>,
) -> Result<DeviceEdgeSession> {
    install_device_bases(cx)?;
    spec.placement
        .validate()
        .map_err(|error| KernelError::HostError(error.to_string()))?;
    let mut session = session;
    if session.profile() != &spec.profile {
        return Err(KernelError::HostError(format!(
            "device provider profile {} did not match requested profile {}",
            session.profile().device,
            spec.profile.device
        )));
    }
    session.start()?;
    let adapter_loop =
        DeviceAdapterLoopPlan::for_profile(&spec.profile, spec.route.clone(), spec.stale);
    let hub_join = DeviceSurfaceHubJoin::new(
        spec.route.clone(),
        spec.profile.device.clone(),
        spec.placement.adapter.symbol.clone(),
    );
    Ok(DeviceEdgeSession {
        spec,
        provider_kind,
        session,
        adapter_loop,
        hub_join,
        live: true,
    })
}
