mod contracts;
mod phase1_red_ok;

/// The witness for an incarnation no mob runtime runs, for unit tests that
/// build member configs or grants without a running mob.
// The crate's one test witness site, listed in the facade's
// host_launch_sites test (`WITNESS_SITES`).
#[allow(clippy::disallowed_methods)]
pub(crate) fn fresh_live_incarnation() -> meerkat_core::service::LiveMobIncarnation {
    meerkat_core::service::LiveMobIncarnation::witnessed(
        meerkat_core::service::MobIncarnationId::mint(),
    )
}
