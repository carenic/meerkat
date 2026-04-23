//! `RecomputeMobPeerOverlay` composition driver.
//!
//! Track-B (R5) Commit 4: the first real consumer of the composition-
//! driver execution framework (`composition_dispatch`). Watches three
//! effect signals from MobMachine and MeerkatMachine and computes,
//! per session, the set of peer endpoints that session should trust
//! based on the identity-level wiring graph owned by MobMachine.
//!
//! Model
//! -----
//!
//! Two graph layers:
//!
//! 1. **Member wiring graph** (`MobMachine`, identity-level): an
//!    undirected set of `WiringEdge { a: AgentIdentity, b:
//!    AgentIdentity }` pairs. Edges are identity-level — they survive
//!    runtime respawn of either endpoint. Source of truth:
//!    `MobMachine.wiring_edges`.
//! 2. **Session peer graph** (`MeerkatMachine`, endpoint-level): per
//!    session, the set of effective trusted peer endpoints. Source
//!    of truth per session: `MeerkatMachine.direct_peer_endpoints ∪
//!    MeerkatMachine.mob_overlay_peer_endpoints`.
//!
//! Projection rule (per bound member M):
//!
//! ```text
//! overlay(M) = {
//!   published_endpoint(other_member_session)
//!   for other_member in wiring_edges.neighbors(M.identity)
//!   if member_session_bindings.contains(other_member)
//!   and local_endpoint.contains(other_member_session)
//! }
//! ```
//!
//! The driver maintains three caches that mirror the authoritative
//! state:
//!
//! * `wiring_edges: BTreeSet<(AgentIdentity, AgentIdentity)>`
//! * `member_session: BTreeMap<AgentIdentity, SessionId>`
//! * `session_endpoint: BTreeMap<SessionId, PeerEndpoint>`
//!
//! Caches are updated via the "observe" methods the dispatcher calls
//! on each watched effect. The driver exposes a pure `recompute_all`
//! method that returns the set of `(SessionId, MobPeerOverlay)`
//! dispatches to route after the caches settle.
//!
//! Driver epoch
//! ------------
//!
//! Each overlay dispatch carries a monotonic `epoch` that the driver
//! advances on every recompute. The epoch threads through to
//! `MeerkatMachine.peer_projection_epoch` via `ApplyMobPeerOverlay`,
//! so out-of-order overlay deliveries are rejected by the
//! `stale_overlay_epoch` guard on the receiving side.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use crate::meerkat_machine::dsl as mm_dsl;

/// Agent identity in the mob-level wiring graph. Mirrors
/// `meerkat_mob::machines::mob_machine::AgentIdentity`; the driver
/// is structurally decoupled from that crate (it is pure Rust that
/// operates on typed newtypes), but the identity strings compare
/// equal across the two crates.
pub type AgentIdentity = String;

/// Session id for the per-session peer overlay. Strings are the
/// common shape across `meerkat_mob::machines::mob_machine::SessionId`
/// and `meerkat_runtime::meerkat_machine::dsl::SessionId`; the driver
/// passes them through unchanged to the
/// `ApplyMobPeerOverlay { epoch, endpoints }` input it dispatches to
/// each `MeerkatMachine` instance.
pub type SessionId = String;

/// Post-recompute dispatch plan for a single bound session.
///
/// The driver emits one of these per bound session on every
/// recompute. `endpoints` is the new overlay set the session should
/// see; empty sets are legal and mean "this session's mob overlay is
/// now empty" (e.g. after its peer retires).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobPeerOverlayDispatch {
    pub session_id: SessionId,
    pub epoch: u64,
    pub endpoints: BTreeSet<mm_dsl::PeerEndpoint>,
}

/// Stateful peer-overlay driver.
///
/// Thread-safe; caches guarded by a single `Mutex`. Observers and
/// `recompute_all` are cheap (O(wiring_edges) worst-case).
#[derive(Debug, Default)]
pub struct RecomputeMobPeerOverlayDriver {
    inner: Mutex<DriverState>,
}

#[derive(Debug, Default)]
struct DriverState {
    /// Undirected wiring graph, stored as ordered pairs (a <= b).
    wiring_edges: BTreeSet<(AgentIdentity, AgentIdentity)>,
    /// Identity → current session id. Mirrors
    /// `MobMachine.member_session_bindings`.
    member_session: BTreeMap<AgentIdentity, SessionId>,
    /// Session → published local endpoint. Mirrors
    /// `MeerkatMachine.local_endpoint` per session. Absent entries
    /// mean the session has not yet published an endpoint (or has
    /// cleared it); the driver skips neighbors with no endpoint.
    session_endpoint: BTreeMap<SessionId, mm_dsl::PeerEndpoint>,
    /// Monotonic driver epoch. Advances on every call to
    /// `recompute_all` that emits a non-empty dispatch list.
    driver_epoch: u64,
    /// Overlay cache: the last overlay the driver routed to each
    /// session. Used to suppress redundant dispatches when
    /// observations change caches but the effective overlay for a
    /// session is unchanged.
    last_overlay_for_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>>,
}

impl RecomputeMobPeerOverlayDriver {
    /// Construct an empty driver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonicalise an (a, b) pair to (min, max) for undirected
    /// edge equality.
    fn canon_edge(a: AgentIdentity, b: AgentIdentity) -> (AgentIdentity, AgentIdentity) {
        if a <= b { (a, b) } else { (b, a) }
    }

    /// Record that a new wiring edge was observed (MobMachine
    /// `WireMembers` transition). Idempotent — re-observing the
    /// same edge is a no-op.
    pub fn observe_wire(&self, a: AgentIdentity, b: AgentIdentity) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.wiring_edges.insert(Self::canon_edge(a, b));
    }

    /// Record that an existing wiring edge was removed (MobMachine
    /// `UnwireMembers` transition). Idempotent — removing an absent
    /// edge is a no-op.
    pub fn observe_unwire(&self, a: AgentIdentity, b: AgentIdentity) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.wiring_edges.remove(&Self::canon_edge(a, b));
    }

    /// Record that a member's session binding was added, rotated, or
    /// released (MobMachine `MemberSessionBindingChanged` effect,
    /// the union of `Set`/`Rotated`/`Released` fine-grained variants).
    ///
    /// `old_session_id` is informational; the driver tracks only the
    /// current binding. `new_session_id == None` means release.
    pub fn observe_binding_change(
        &self,
        agent_identity: AgentIdentity,
        _old_session_id: Option<SessionId>,
        new_session_id: Option<SessionId>,
    ) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match new_session_id {
            Some(sid) => {
                guard.member_session.insert(agent_identity, sid);
            }
            None => {
                guard.member_session.remove(&agent_identity);
            }
        }
    }

    /// Record that a session published (or updated) its local
    /// endpoint (MeerkatMachine `LocalEndpointChanged` with
    /// `Some(endpoint)`).
    pub fn observe_local_endpoint(
        &self,
        session_id: SessionId,
        endpoint: Option<mm_dsl::PeerEndpoint>,
    ) {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match endpoint {
            Some(ep) => {
                guard.session_endpoint.insert(session_id, ep);
            }
            None => {
                guard.session_endpoint.remove(&session_id);
            }
        }
    }

    /// Recompute overlays for every bound session and return the
    /// dispatch plan.
    ///
    /// Only sessions whose computed overlay **changed** since the
    /// previous recompute are included in the returned vector; this
    /// keeps the stale-epoch guard on `ApplyMobPeerOverlay` from
    /// rejecting redundant no-op deliveries (the driver advances the
    /// epoch on every real change, so stale-rejected no-op deliveries
    /// would leave the session's `peer_projection_epoch` stuck).
    ///
    /// The returned epochs are strictly increasing within a single
    /// `recompute_all` call: multiple sessions whose overlays change
    /// in the same recompute get distinct epoch values so callers
    /// observe a total order of overlay deliveries.
    pub fn recompute_all(&self) -> Vec<MobPeerOverlayDispatch> {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Collect neighbor lists per identity from the wiring graph.
        let mut neighbors: BTreeMap<AgentIdentity, BTreeSet<AgentIdentity>> = BTreeMap::new();
        for (a, b) in &guard.wiring_edges {
            neighbors.entry(a.clone()).or_default().insert(b.clone());
            neighbors.entry(b.clone()).or_default().insert(a.clone());
        }

        // For each bound member, project neighbor identities onto their
        // published endpoints.
        let mut per_session_overlay: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> =
            BTreeMap::new();
        for (identity, session) in &guard.member_session {
            let overlay: BTreeSet<mm_dsl::PeerEndpoint> = neighbors
                .get(identity)
                .into_iter()
                .flat_map(|set| set.iter())
                .filter_map(|neighbor_identity| guard.member_session.get(neighbor_identity))
                .filter_map(|neighbor_session| guard.session_endpoint.get(neighbor_session))
                .cloned()
                .collect();
            per_session_overlay.insert(session.clone(), overlay);
        }

        // Sessions previously had a non-empty overlay but their member
        // has been released — emit an empty overlay so the session
        // drops stale trust.
        let previously_bound_sessions: BTreeSet<SessionId> =
            guard.last_overlay_for_session.keys().cloned().collect();
        for previous_session in previously_bound_sessions {
            per_session_overlay.entry(previous_session).or_default();
        }

        // Filter out sessions whose overlay is unchanged since last
        // recompute; this avoids spurious dispatches.
        let mut dispatches = Vec::new();
        for (session_id, overlay) in per_session_overlay {
            let prior = guard.last_overlay_for_session.get(&session_id);
            if prior.map(|p| p == &overlay).unwrap_or(false) {
                continue;
            }
            // Update cache and emit.
            guard.driver_epoch += 1;
            let epoch = guard.driver_epoch;
            guard
                .last_overlay_for_session
                .insert(session_id.clone(), overlay.clone());
            dispatches.push(MobPeerOverlayDispatch {
                session_id,
                epoch,
                endpoints: overlay,
            });
        }

        // Garbage-collect cache entries for sessions that are no
        // longer bound to any member. We MUST keep empty-overlay
        // entries for still-bound sessions so the next recompute
        // sees `prior == current` and short-circuits — otherwise
        // the driver would dispatch an empty overlay on every call
        // for any session whose neighbors have no endpoints (an
        // infinite loop). Only bindings leaving the `member_session`
        // map cause legitimate GC.
        let bound_sessions: BTreeSet<SessionId> = guard.member_session.values().cloned().collect();
        guard
            .last_overlay_for_session
            .retain(|session_id, _| bound_sessions.contains(session_id));

        dispatches
    }

    /// Current driver epoch (for observability / test assertions).
    pub fn epoch(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .driver_epoch
    }

    /// Snapshot of the wiring edges the driver has observed (for
    /// shadow-mode parity tests).
    #[cfg(test)]
    pub(crate) fn wiring_snapshot(&self) -> BTreeSet<(AgentIdentity, AgentIdentity)> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .wiring_edges
            .clone()
    }

    /// Snapshot of the member→session bindings the driver has
    /// observed (for shadow-mode parity tests).
    #[cfg(test)]
    pub(crate) fn bindings_snapshot(&self) -> BTreeMap<AgentIdentity, SessionId> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .member_session
            .clone()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn identity(name: &str) -> AgentIdentity {
        name.to_string()
    }

    fn sid(name: &str) -> SessionId {
        name.to_string()
    }

    fn endpoint(name: &str) -> mm_dsl::PeerEndpoint {
        mm_dsl::PeerEndpoint {
            name: format!("ep-{name}"),
            peer_id: format!("ed25519:{name}"),
            address: format!("inproc://{name}"),
        }
    }

    #[test]
    fn wired_m1_m2_bound_to_distinct_sessions_routes_cross_endpoints() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));

        let dispatches = driver.recompute_all();
        assert_eq!(dispatches.len(), 2, "one dispatch per bound session");

        let mut by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = BTreeMap::new();
        for dispatch in &dispatches {
            by_session.insert(dispatch.session_id.clone(), dispatch.endpoints.clone());
        }
        assert_eq!(
            by_session.get(&sid("S1")),
            Some(&BTreeSet::from([endpoint("E2")])),
            "S1 overlay must be {{E2}} — M1's only wired neighbor is M2, whose session is S2 with endpoint E2",
        );
        assert_eq!(
            by_session.get(&sid("S2")),
            Some(&BTreeSet::from([endpoint("E1")])),
            "S2 overlay must be {{E1}} by symmetry",
        );

        // Epochs are strictly increasing.
        let mut seen = BTreeSet::new();
        for dispatch in &dispatches {
            assert!(seen.insert(dispatch.epoch), "epochs must be unique");
        }
    }

    #[test]
    fn respawn_of_m1_rotates_to_new_session_and_clears_prior_session_overlay() {
        // Setup: M1↔M2 wired, M1 bound to S1 with E1, M2 bound to S2
        // with E2.
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let _ = driver.recompute_all();

        // Respawn: M1 is rotated from S1 to S1' and S1' publishes E1'.
        // The driver observes rotate (old=S1, new=S1') followed by the
        // new local endpoint for S1'.
        driver.observe_binding_change(identity("M1"), Some(sid("S1")), Some(sid("S1_new")));
        driver.observe_local_endpoint(sid("S1_new"), Some(endpoint("E1_new")));
        // S1's local endpoint is also cleared on the old session's
        // teardown.
        driver.observe_local_endpoint(sid("S1"), None);

        let dispatches = driver.recompute_all();
        let by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = dispatches
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();

        assert_eq!(
            by_session.get(&sid("S1_new")),
            Some(&BTreeSet::from([endpoint("E2")])),
            "S1' overlay must be {{E2}} — M1 (now bound to S1') is still wired to M2 whose session S2 carries E2",
        );
        assert_eq!(
            by_session.get(&sid("S2")),
            Some(&BTreeSet::from([endpoint("E1_new")])),
            "S2 overlay must rotate to {{E1'}} — M2 is still wired to M1 whose session is now S1' with E1'",
        );
        assert_eq!(
            by_session.get(&sid("S1")),
            Some(&BTreeSet::new()),
            "S1 must get an empty overlay to drop stale E2 trust now that S1 is no longer a bound member's session",
        );
    }

    #[test]
    fn unwire_clears_mutual_overlays() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let _ = driver.recompute_all();

        driver.observe_unwire(identity("M1"), identity("M2"));

        let dispatches = driver.recompute_all();
        let by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = dispatches
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();

        assert_eq!(
            by_session.get(&sid("S1")),
            Some(&BTreeSet::new()),
            "S1 must get empty overlay after unwire",
        );
        assert_eq!(
            by_session.get(&sid("S2")),
            Some(&BTreeSet::new()),
            "S2 must get empty overlay after unwire",
        );
    }

    #[test]
    fn idempotent_recompute_emits_no_dispatch_when_caches_unchanged() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let first = driver.recompute_all();
        assert!(!first.is_empty());
        let epoch_after_first = driver.epoch();

        // Second recompute with no cache mutations must emit nothing
        // and must not advance the driver epoch.
        let second = driver.recompute_all();
        assert!(
            second.is_empty(),
            "idempotent recompute must emit no dispatches when cached overlays are unchanged",
        );
        assert_eq!(
            driver.epoch(),
            epoch_after_first,
            "driver epoch must not advance when no overlays change",
        );
    }

    #[test]
    fn empty_overlay_for_still_bound_session_does_not_loop_forever() {
        // Regression for PR #340 review item #1: a bound session
        // whose overlay is legitimately empty (e.g. all neighbors
        // have released their bindings) must not re-dispatch the
        // empty overlay on every `recompute_all` call. The GC must
        // keep empty-overlay cache entries for still-bound sessions
        // so the next recompute sees `prior == current` and
        // short-circuits.
        let driver = RecomputeMobPeerOverlayDriver::new();
        // M1 is bound with an endpoint but has no wired neighbors —
        // overlay is empty.
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        let first = driver.recompute_all();
        assert_eq!(
            first.len(),
            1,
            "first recompute emits the empty overlay once"
        );
        assert_eq!(
            first[0].endpoints,
            BTreeSet::new(),
            "overlay must be empty (no neighbors)",
        );

        // Subsequent recomputes with no cache mutations must NOT
        // emit anything — S1 is still bound, so its empty-overlay
        // cache entry is preserved by the GC, and the idempotency
        // check short-circuits.
        let second = driver.recompute_all();
        assert!(
            second.is_empty(),
            "still-bound session with unchanged empty overlay must not re-dispatch",
        );
        let third = driver.recompute_all();
        assert!(
            third.is_empty(),
            "still-bound session with unchanged empty overlay must stay silent",
        );

        // Sanity: if the session leaves the binding map, GC removes
        // its cache entry and a future re-bind delivers a fresh
        // overlay.
        driver.observe_binding_change(identity("M1"), Some(sid("S1")), None);
        let _ = driver.recompute_all(); // emits final empty overlay for S1 and GCs.
    }

    #[test]
    fn re_wire_same_edge_is_idempotent() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_wire(identity("M1"), identity("M2")); // re-wire
        driver.observe_wire(identity("M2"), identity("M1")); // reversed

        let snapshot = driver.wiring_snapshot();
        assert_eq!(
            snapshot.len(),
            1,
            "wiring set must carry exactly one canonicalised edge for M1↔M2",
        );
    }

    #[test]
    fn release_binding_removes_member_from_recompute() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let _ = driver.recompute_all();

        // Release M1's binding.
        driver.observe_binding_change(identity("M1"), Some(sid("S1")), None);

        let bindings = driver.bindings_snapshot();
        assert!(
            !bindings.contains_key(&identity("M1")),
            "released identity must no longer be in the binding cache",
        );

        let dispatches = driver.recompute_all();
        let by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = dispatches
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();
        // M2's overlay: M1 no longer bound, so no endpoint to project.
        assert_eq!(
            by_session.get(&sid("S2")),
            Some(&BTreeSet::new()),
            "S2 must get empty overlay after M1's binding is released",
        );
        // S1 was the prior-bound session; overlay must clear.
        assert_eq!(
            by_session.get(&sid("S1")),
            Some(&BTreeSet::new()),
            "S1 must get empty overlay after M1's binding is released",
        );
    }

    /// Shadow-mode parity test: drive the driver through the same
    /// sequence actor.rs's `restore_wiring` code at
    /// `meerkat-mob/src/runtime/actor.rs:4299-4331` handles on
    /// respawn, and verify the driver produces an equivalent
    /// effective trust set.
    ///
    /// Actor.rs today rebuilds peer wiring imperatively after a
    /// respawn: it iterates the prior roster entry's
    /// `RestoreWiringPlan.local_peers` + `external_peers` and
    /// replays `do_wire` / `do_wire_external` to reinstall each
    /// trust edge on the new member's session.
    ///
    /// The driver replaces this by: (a) keeping the wiring graph
    /// identity-level (no respawn restoration needed; the edge was
    /// never torn down), and (b) rotating the session-binding map
    /// on `Respawn`, which triggers a recompute that routes the
    /// updated session's endpoint to every neighbor.
    ///
    /// Parity assertion: for a wired M1↔M2 mob where M1 respawns
    /// from S1 → S1' with a new endpoint E1', both the driver and
    /// actor.rs's shell code arrive at the same effective trust
    /// view:
    ///
    /// ```text
    /// S1' trusts {E2}       — M1's neighbor M2 is still on E2
    /// S2  trusts {E1'}      — M2's neighbor M1 is now on E1'
    /// S1  trusts {}         — session torn down; trust drops
    /// ```
    ///
    /// Commit 5 removes the actor.rs restore path; this test pins
    /// the parity the driver must preserve through that cutover.
    #[test]
    fn shadow_mode_parity_matches_actor_restore_wiring_after_respawn() {
        let driver = RecomputeMobPeerOverlayDriver::new();

        // Phase 1: initial wiring + binding + endpoints. The actor
        // and DSL both land at the same state after spawn of M1 and
        // M2 + external wire.
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let initial = driver.recompute_all();
        let initial_by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = initial
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();
        assert_eq!(
            initial_by_session.get(&sid("S1")),
            Some(&BTreeSet::from([endpoint("E2")])),
            "initial S1 overlay: {{E2}}",
        );
        assert_eq!(
            initial_by_session.get(&sid("S2")),
            Some(&BTreeSet::from([endpoint("E1")])),
            "initial S2 overlay: {{E1}}",
        );

        // Phase 2: simulate respawn of M1.
        // Actor.rs path: roster `upsert_external_binding_overlay` +
        // `handle_respawn` rotates the runtime id; at DSL level the
        // `Spawn` transition fires `MemberSessionBindingRotated` for
        // M1's identity with `old=S1, new=S1'`. Meerkat's `S1_new`
        // session publishes E1' via `PublishLocalEndpoint`. S1
        // closes via `ClearLocalEndpoint` on the archived session.
        driver.observe_binding_change(identity("M1"), Some(sid("S1")), Some(sid("S1_new")));
        driver.observe_local_endpoint(sid("S1_new"), Some(endpoint("E1_new")));
        driver.observe_local_endpoint(sid("S1"), None);

        // The driver's recompute_all output is the shell-equivalent
        // of what actor.rs's `restore_wiring` installs on the new
        // session + what the comms runtime diffs on the
        // corresponding peers.
        let after_respawn = driver.recompute_all();
        let overlays: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = after_respawn
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();

        // Parity assertion 1: S1' trusts {E2}.
        // actor.rs counterpart: `restore_wiring.local_peers = [M2]`
        // → `do_wire(M1, M2)` on the new session, which resolves M2
        // to its current endpoint E2.
        assert_eq!(
            overlays.get(&sid("S1_new")),
            Some(&BTreeSet::from([endpoint("E2")])),
            "S1' must trust {{E2}} (actor.rs parity: do_wire(M1, M2) after respawn)",
        );

        // Parity assertion 2: S2 trusts {E1'}.
        // actor.rs counterpart: the respawn path reinstalls trust
        // on M2's session for the new M1 endpoint — in actor.rs
        // today this is done via the peer-notification path that
        // updates M2's trust when M1 respawns. The driver achieves
        // the same result by recomputing M2's overlay.
        assert_eq!(
            overlays.get(&sid("S2")),
            Some(&BTreeSet::from([endpoint("E1_new")])),
            "S2 must trust {{E1'}} (actor.rs parity: M2 rewired to the new M1 endpoint)",
        );

        // Parity assertion 3: S1 (the archived session) trusts {}.
        // actor.rs counterpart: the archived session's comms
        // runtime is torn down; no wiring is preserved on it.
        assert_eq!(
            overlays.get(&sid("S1")),
            Some(&BTreeSet::new()),
            "S1 (archived session) must trust no one — parity with actor.rs session teardown",
        );
    }

    #[test]
    fn clearing_a_neighbors_endpoint_clears_peer_overlay_entry() {
        let driver = RecomputeMobPeerOverlayDriver::new();
        driver.observe_wire(identity("M1"), identity("M2"));
        driver.observe_binding_change(identity("M1"), None, Some(sid("S1")));
        driver.observe_binding_change(identity("M2"), None, Some(sid("S2")));
        driver.observe_local_endpoint(sid("S1"), Some(endpoint("E1")));
        driver.observe_local_endpoint(sid("S2"), Some(endpoint("E2")));
        let _ = driver.recompute_all();

        // S2 clears its local endpoint.
        driver.observe_local_endpoint(sid("S2"), None);

        let dispatches = driver.recompute_all();
        let by_session: BTreeMap<SessionId, BTreeSet<mm_dsl::PeerEndpoint>> = dispatches
            .iter()
            .map(|d| (d.session_id.clone(), d.endpoints.clone()))
            .collect();
        assert_eq!(
            by_session.get(&sid("S1")),
            Some(&BTreeSet::new()),
            "S1 must lose its overlay entry now that S2 has no local endpoint",
        );
    }
}
