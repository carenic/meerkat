// @generated — protocol helpers for `auth_lease_lifecycle_publication`
// Composition: auth_lease_bundle, Producer: auth_lease_bridge, Effect: PublishLifecycleEvent
// Closure policy: AckRequired
// Liveness: informative publication: AuthMachine's own transitions carry the authoritative phase fact; runtime owner refreshes the lease-state projection under task-scheduling fairness

use crate::handles::auth_lease::AuthLeaseBridgeEffect;

#[derive(Debug, Clone)]
pub struct AuthLeaseLifecyclePublicationObligation {
    pub new_state: AuthLifecyclePhase,
}

pub fn extract_obligations(
    effects: &[AuthLeaseBridgeEffect],
) -> Vec<AuthLeaseLifecyclePublicationObligation> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            AuthLeaseBridgeEffect::PublishLifecycleEvent { new_state } => {
                Some(AuthLeaseLifecyclePublicationObligation {
                    new_state: new_state.clone(),
                })
            }
            _ => None,
        })
        .collect()
}
