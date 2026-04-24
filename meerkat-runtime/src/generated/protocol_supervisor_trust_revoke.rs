// @generated — protocol helpers for `supervisor_trust_revoke`
// Composition: supervisor_trust_bundle, Producer: supervisor_trust_bridge, Effect: RevokeSupervisorTrustEdge
// Closure policy: AckRequired
// Liveness: eventual feedback under comms transport liveness — `send_bridge_response` surfaces the typed outcome

use crate::comms_drain::SupervisorTrustBridgeEffect;

#[derive(Debug, Clone)]
pub struct SupervisorTrustRevokeObligation {
    pub peer_id: SupervisorPeerId,
    pub epoch: u64,
}

pub fn extract_obligations(
    effects: &[SupervisorTrustBridgeEffect],
) -> Vec<SupervisorTrustRevokeObligation> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            SupervisorTrustBridgeEffect::RevokeSupervisorTrustEdge { peer_id, epoch } => {
                Some(SupervisorTrustRevokeObligation {
                    peer_id: peer_id.clone(),
                    epoch: *epoch,
                })
            }
            _ => None,
        })
        .collect()
}
