//! Observation of an original work's registration's live MCP adapter.
//!
//! The retained original selects the registration to observe; it authorizes
//! nothing. The answer describes what that registration's current local MCP
//! router exposes for one exact raw operation now, and is withheld unless
//! the original's run destination is still current. It never grants a call.

use meerkat_core::McpOperationObservation;
use meerkat_core::retained_work::RetainedWorkIdentity;
use meerkat_core::types::SessionId;

use super::scoped_context::run_destination_current;
use super::{MeerkatMachine, dsl};

/// Why an original work's MCP observation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum McpObservationRefusal {
    /// The original's runtime has no live registration here.
    #[error("the original work's registration is not live")]
    Retired,
    /// The original's registration, or its observed installation, changed
    /// while it was being observed.
    #[error("the original work's registration changed")]
    Rebound,
    /// The original's retained rows are not exactly present here.
    #[error("the original work is not current here")]
    OriginalUnavailable,
    /// The original run's destination is no longer current.
    #[error("the original work's destination is not current")]
    DestinationWithheld,
    /// The registration has no finished construction's MCP adapter.
    #[error("the original work's registration has no observed MCP adapter")]
    NotInstalled,
}

impl MeerkatMachine {
    /// What the MCP adapter of the registration that holds `original` exposes
    /// now for `server`'s exact raw operation `raw_operation`.
    ///
    /// The registration is the live one whose runtime staged the original;
    /// its retained rows are checked exactly, and the original run's
    /// destination must still be current. The registration and its observed
    /// installation are checked again after the router is read, so a rebuild,
    /// re-registration or retirement in between is refused, not reported.
    pub async fn observe_original_mcp_operation(
        &self,
        original: &RetainedWorkIdentity,
        server: &str,
        raw_operation: &str,
    ) -> Result<McpOperationObservation, McpObservationRefusal> {
        let (session_id, slot) = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .find(|(_, entry)| entry.runtime_id.to_string() == original.runtime_id())
                .map(|(session_id, entry)| (session_id.clone(), entry.mcp_observation.clone()))
                .ok_or(McpObservationRefusal::Retired)?
        };
        self.resolve_retained_work_contributors(&session_id, original)
            .await
            .map_err(|_| McpObservationRefusal::OriginalUnavailable)?;
        self.require_current_observation_target(&session_id, &slot, original)
            .await?;
        let (installation, observer) = slot.current().ok_or(McpObservationRefusal::NotInstalled)?;
        // The installed adapter went with the agent that owned it: its router
        // has been released.
        let observation = match observer {
            Some(observer) => observer.observe_operation(server, raw_operation).await,
            None => McpOperationObservation::Retired,
        };
        self.require_current_observation_target(&session_id, &slot, original)
            .await?;
        if !slot.is_current(installation) {
            return Err(McpObservationRefusal::Rebound);
        }
        Ok(observation)
    }

    /// The registration still holds `slot` and the original run's
    /// destination is still current.
    async fn require_current_observation_target(
        &self,
        session_id: &SessionId,
        slot: &meerkat_core::McpObservationSlot,
        original: &RetainedWorkIdentity,
    ) -> Result<(), McpObservationRefusal> {
        let driver = {
            let sessions = self.sessions.read().await;
            let entry = sessions
                .get(session_id)
                .ok_or(McpObservationRefusal::Retired)?;
            if !entry.mcp_observation.same_slot(slot) {
                return Err(McpObservationRefusal::Rebound);
            }
            entry.driver.clone()
        };
        let driver = driver.lock().await;
        let authority = driver.shared_dsl_authority();
        let authority = authority
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = authority.state();
        let run = dsl::RunId::from_domain(original.run_id());
        // Naming the currently staged run must be that run's own work: its
        // selected inputs are associated with it. Any other run's destination
        // reads the same for every identity naming it. This only keeps a
        // claimed run id from widening this observation; it does not prove
        // the run was admitted or its batch complete, and permits nothing.
        if state.authority_staged_run.as_ref() == Some(&run)
            && (original.selected_input_bindings().is_empty()
                || original
                    .selected_input_bindings()
                    .keys()
                    .any(|key| state.input_run_associations.get(key) != Some(&run)))
        {
            return Err(McpObservationRefusal::OriginalUnavailable);
        }
        if run_destination_current(state, &run) {
            Ok(())
        } else {
            Err(McpObservationRefusal::DestinationWithheld)
        }
    }
}
