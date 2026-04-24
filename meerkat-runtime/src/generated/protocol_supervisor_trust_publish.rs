// @generated — protocol helpers for `supervisor_trust_publish`
// Composition: supervisor_trust_bundle, Producer: supervisor_trust_bridge, Effect: PublishSupervisorTrustEdge
// Closure policy: AckRequired
// Liveness: eventual feedback under comms transport liveness — `send_bridge_response` surfaces the typed outcome

use crate::comms_drain::SupervisorTrustBridgeEffect;

#[derive(Debug, Clone)]
pub struct SupervisorTrustPublishObligation {
    pub peer_id: SupervisorPeerId,
    pub name: String,
    pub address: String,
    pub epoch: u64,
}

pub fn extract_obligations(
    effects: &[SupervisorTrustBridgeEffect],
) -> Vec<SupervisorTrustPublishObligation> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            SupervisorTrustBridgeEffect::PublishSupervisorTrustEdge {
                peer_id,
                name,
                address,
                epoch,
            } => Some(SupervisorTrustPublishObligation {
                peer_id: peer_id.clone(),
                name: name.clone(),
                address: address.clone(),
                epoch: *epoch,
            }),
            _ => None,
        })
        .collect()
}
