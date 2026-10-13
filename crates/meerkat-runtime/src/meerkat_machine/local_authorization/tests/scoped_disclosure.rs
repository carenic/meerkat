//! Result disclosure of scoped work: the admitted row keeps its original
//! destination witness, and a result reader receives the payload only while
//! that exact destination is the session document's current binding.
use super::*;
use crate::input_state::{ScopedDestinationWitness, StoredInputState};
use crate::meerkat_machine::scoped_context::{
    ScopedHostPosture, disclosure_permitted_with, result_disclosure_permitted, scoped_host_posture,
};
use meerkat_authorization_contracts::work_association::NativeWorkTarget;

fn target_of(input: &Input) -> NativeWorkTarget {
    input
        .header()
        .authority_association
        .as_ref()
        .expect("claims")
        .candidate()
        .target
        .clone()
}

async fn accept_and_read_row(
    machine: &MeerkatMachine,
    session: &SessionId,
    input: Input,
) -> StoredInputState {
    let input_id = input.id().clone();
    let driver = Arc::clone(
        &machine
            .sessions
            .read()
            .await
            .get(session)
            .expect("entry")
            .driver,
    );
    let mut locked = driver.lock().await;
    let DriverEntry::Ephemeral(driver) = &mut *locked else {
        panic!("storeless")
    };
    driver.set_executor_work_authorization_support(true);
    driver.accept_input(input).await.expect("admitted");
    driver.stored_input_state(&input_id).expect("admitted row")
}

async fn authority(
    machine: &MeerkatMachine,
    session: &SessionId,
) -> crate::driver::ephemeral::SharedIngressDslAuthority {
    let driver = Arc::clone(
        &machine
            .sessions
            .read()
            .await
            .get(session)
            .expect("entry")
            .driver,
    );
    let locked = driver.lock().await;
    locked.shared_dsl_authority()
}

/// The row of work admitted under a bound destination records that exact
/// destination. Its result is disclosable only while that destination is
/// current: not once it is retired, not after a rebind of the same scope to
/// the next generation (currentness of some destination is not enough), and
/// not without a live registration (a detached read has no current
/// destination). The witness survives the row's wire encoding.
#[tokio::test]
async fn the_original_destination_witness_decides_result_disclosure() {
    let (configuration, prompt, _domain) = configuration();
    let (machine, session, prompt) = pending_controller_input(configuration, prompt).await;
    install_owner_fixture_credential(&machine, &prompt);
    let registration = machine
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    let target = target_of(&prompt);
    machine
        .bind_scoped_context(&registration, &target)
        .await
        .expect("bind the destination");
    let row = accept_and_read_row(&machine, &session, prompt).await;
    let binding = crate::input_authority::context_target_binding(&target).expect("binding");
    assert_eq!(
        row.state.scoped_destination,
        ScopedDestinationWitness::Bound {
            binding: binding.clone()
        },
        "the admitted row keeps the destination it was admitted under"
    );
    let wire = serde_json::to_value(&row).expect("row encoding");
    let decoded: StoredInputState = serde_json::from_value(wire).expect("row decoding");
    assert_eq!(
        decoded.state.scoped_destination,
        row.state.scoped_destination
    );

    let posture = scoped_host_posture(&machine.native_work_authorization_host);
    assert_eq!(posture, ScopedHostPosture::Governed);
    let live = authority(&machine, &session).await;
    assert!(
        disclosure_permitted_with(Some(&live), &row.state, posture),
        "the current destination receives its result"
    );
    assert!(
        !disclosure_permitted_with(None, &row.state, posture),
        "a read without a live registration has no current destination"
    );

    machine
        .retire_scoped_context(&registration, target.context_generation)
        .await
        .expect("retire");
    assert!(
        !disclosure_permitted_with(Some(&live), &row.state, posture),
        "a retired destination receives nothing"
    );
    let mut next = target.clone();
    next.context_generation += 1;
    machine
        .bind_scoped_context(&registration, &next)
        .await
        .expect("rebind the same scope at the next generation");
    assert!(
        !disclosure_permitted_with(Some(&live), &row.state, posture),
        "another (current) destination of the same scope receives nothing"
    );
}

/// A row without the witness (written before admissions recorded it, or
/// by a path that does not) is never taken as unscoped governed work: with a
/// governed host its result is withheld, attached or detached, even on a
/// document that never served a destination. Ungoverned legacy rows keep
/// their previous disclosure on such a document; a machine without a
/// governed host can bind no destination and discloses as before.
#[tokio::test]
async fn a_missing_witness_never_means_never_scoped() {
    let (configuration, prompt, _domain) = configuration();
    let (machine, session, prompt) = pending_controller_input(configuration, prompt).await;
    install_owner_fixture_credential(&machine, &prompt);
    let row = accept_and_read_row(&machine, &session, prompt).await;
    assert_eq!(
        row.state.scoped_destination,
        ScopedDestinationWitness::NeverScoped,
        "governed work on a never-scoped document records that"
    );
    let mut wire = serde_json::to_value(&row).expect("row encoding");
    assert!(
        wire.as_object_mut()
            .expect("row object")
            .remove("scoped_destination")
            .is_some()
    );
    let legacy: StoredInputState = serde_json::from_value(wire).expect("legacy row decoding");
    assert_eq!(
        legacy.state.scoped_destination,
        ScopedDestinationWitness::Unrecorded
    );
    assert!(
        !legacy.state.authority_contributors.is_empty(),
        "governed row"
    );

    let posture = scoped_host_posture(&machine.native_work_authorization_host);
    let live = authority(&machine, &session).await;
    assert!(
        disclosure_permitted_with(Some(&live), &row.state, posture),
        "recorded never-scoped work on a never-scoped document is disclosed"
    );
    assert!(
        !disclosure_permitted_with(Some(&live), &legacy.state, posture),
        "an unrecorded governed row is withheld while attached"
    );
    assert!(
        !disclosure_permitted_with(None, &legacy.state, posture),
        "an unrecorded governed row is withheld while detached"
    );
    {
        let live = live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = live.state();
        assert!(
            result_disclosure_permitted(
                Some(state),
                &ScopedDestinationWitness::Unrecorded,
                false,
                posture,
            ),
            "an unrecorded ungoverned row on a never-scoped document"
        );
        assert!(
            result_disclosure_permitted(
                Some(state),
                &ScopedDestinationWitness::Unrecorded,
                true,
                ScopedHostPosture::Ungoverned,
            ),
            "no governed host: no destination can exist here"
        );
        assert!(
            !result_disclosure_permitted(
                Some(state),
                &ScopedDestinationWitness::NeverScoped,
                true,
                ScopedHostPosture::RequiresDestination,
            ),
            "a host requiring destinations never treats a document as unscoped"
        );
        assert!(
            !result_disclosure_permitted(
                Some(state),
                &ScopedDestinationWitness::Unbound,
                true,
                posture,
            ),
            "work admitted without a destination on a scoped document"
        );
    }
}

/// A completion buffered for later release keeps the generated authority it
/// was delivered under, whose scoped binding survives an unregister. Release
/// is decided against the actual current registration instead: after the
/// session is unregistered nothing is released, and a re-registration (even
/// bound to the same destination) is another registration.
#[tokio::test]
async fn a_buffered_release_is_decided_against_the_current_registration() {
    let (configuration, prompt, _domain) = configuration();
    let (machine, session, prompt) = pending_controller_input(configuration, prompt).await;
    install_owner_fixture_credential(&machine, &prompt);
    let registration = machine
        .current_session_registration_witness(&session)
        .await
        .expect("current registration");
    let target = target_of(&prompt);
    machine
        .bind_scoped_context(&registration, &target)
        .await
        .expect("bind the destination");
    let row = accept_and_read_row(&machine, &session, prompt).await;
    let gate = crate::meerkat_machine::scoped_context::completion_disclosure_gate(
        Arc::downgrade(&machine.shared),
        session.clone(),
        authority(&machine, &session).await,
    );
    let buffered = gate(&row.state.input_id).expect("an admitted input is decided");
    let buffered_again = gate(&row.state.input_id).expect("an admitted input is decided");
    assert!(
        buffered.permitted().await,
        "the current registration's current destination receives its result"
    );
    drop(registration);

    machine
        .unregister_session(&session)
        .await
        .expect("unregister the session");
    assert!(
        !buffered.permitted().await,
        "an unregistered session releases nothing, whatever its old authority still binds"
    );

    machine
        .register_session(session.clone())
        .await
        .expect("register again");
    let registration = machine
        .current_session_registration_witness(&session)
        .await
        .expect("new registration");
    machine
        .bind_scoped_context(&registration, &target)
        .await
        .expect("the new registration binds the same destination");
    assert!(
        !buffered_again.permitted().await,
        "a re-registration is another registration"
    );
}
