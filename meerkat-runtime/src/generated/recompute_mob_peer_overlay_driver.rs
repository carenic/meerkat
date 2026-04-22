// @generated — composition driver descriptor for `meerkat_mob_seam`
// DO NOT EDIT. Emitted by meerkat-machine-codegen::render_composition_driver.
// Source of truth: catalog::compositions::meerkat_mob_seam

use meerkat_runtime::composition_dispatch::*;

/// Logical name of this composition driver. Used at runtime for
/// registration and diagnostics.
pub const DRIVER_NAME: &str = "RecomputeMobPeerOverlay";

/// Effect variants this driver observes.
/// Each entry is `(producer_instance, effect_variant)`.
pub const WATCHED_EFFECTS: &[(&str, &str)] = &[
    ("mob", "WiringGraphChanged"),
    ("mob", "MemberSessionBindingChanged"),
    ("meerkat", "LocalEndpointChanged"),
];

/// Dispatch routes this driver may emit. Each entry is
/// `(route_name, target_instance, target_kind, input_variant)`.
pub const DISPATCH_ROUTES: &[(&str, &str, &str, &str)] = &[(
    "apply_mob_peer_overlay",
    "meerkat",
    "Input",
    "ApplyMobPeerOverlay",
)];

/// Generated marker: the driver type name declared in the schema.
/// The application-side driver must satisfy the
/// `meerkat_runtime::composition_dispatch::CompositionDriverTrait`
/// contract under this name.
pub const DRIVER_TYPE: &str = "RecomputeMobPeerOverlayDriver";
pub const STORE_PLAN_TYPE: &str = "RecomputeMobPeerOverlayStorePlan";
pub const WORK_TYPE: &str = "RecomputeMobPeerOverlayWork";
pub const DECISION_TYPE: &str = "RecomputeMobPeerOverlayDecision";
