// @generated — protocol helpers for `mob_destroying_session_ingress`
// Composition: mob_destroy_session_ingress_bundle, Producer: mob_destroy_session_ingress_bridge, Effect: RequestSessionIngressDetachForMobDestroy
// Closure policy: AckRequired
// Liveness: eventual feedback: the mob destroy path awaits each session's DetachIngress ack before requesting runtime destroy

use crate::runtime::actor::MobDestroySessionIngressBridgeEffect;

#[derive(Debug, Clone)]
pub struct MobDestroyingSessionIngressObligation {
    pub mob_id: MobId,
    pub agent_runtime_id: AgentRuntimeId,
}

pub fn extract_obligations(
    effects: &[MobDestroySessionIngressBridgeEffect],
) -> Vec<MobDestroyingSessionIngressObligation> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            MobDestroySessionIngressBridgeEffect::RequestSessionIngressDetachForMobDestroy {
                mob_id,
                agent_runtime_id,
            } => Some(MobDestroyingSessionIngressObligation {
                mob_id: mob_id.clone(),
                agent_runtime_id: agent_runtime_id.clone(),
            }),
            _ => None,
        })
        .collect()
}
