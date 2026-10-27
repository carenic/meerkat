//! The auth binding a live actor runs, checked before it is reused.
//!
//! A member is rebuilt on the auth binding its current spec names. Where a
//! path would keep a session's live actor instead of rebuilding it (a
//! Resume-mode spawn of a session with a live actor, a restore that keeps a
//! live session, a wake onto a live successor session), the actor is kept
//! only when it runs that binding: a live actor is never reused on another
//! binding.

use meerkat_core::types::SessionId;

use super::session_service::MobSessionService;
use crate::error::MobError;
use crate::ids::AgentIdentity;

/// Whether the live actor of `session_id` runs `requested`, the auth binding
/// the spec of `member` names: the binding its session recorded (persisted
/// when the actor was built, before it served) is `requested` as named, or
/// the form a build of `requested` records (stamped with the realm that
/// declares it, as the session service resolves it). Anything else, a
/// session that recorded no binding included, is
/// [`MobError::MemberAuthBindingConflict`]; a metadata read that fails is
/// its error. A requested binding the service cannot resolve has no other
/// form, so only its named form can match.
pub(super) async fn verify_live_actor_auth_binding(
    session_service: &dyn MobSessionService,
    member: &AgentIdentity,
    session_id: &SessionId,
    requested: &meerkat_core::AuthBindingRef,
) -> Result<(), MobError> {
    let recorded = session_service
        .load_persisted_session_metadata(session_id)
        .await
        .map_err(MobError::SessionError)?
        .and_then(|view| view.session_metadata)
        .and_then(|metadata| metadata.auth_binding);
    if recorded.as_ref() == Some(requested) {
        return Ok(());
    }
    if let Some(recorded_binding) = recorded.as_ref()
        && session_service
            .recorded_auth_binding(requested)
            .await
            .ok()
            .flatten()
            .as_ref()
            == Some(recorded_binding)
    {
        return Ok(());
    }
    Err(MobError::MemberAuthBindingConflict {
        member_id: member.clone(),
        session_id: session_id.clone(),
        requested: requested.clone(),
        recorded,
    })
}
