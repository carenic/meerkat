//! The exact-admission comparison of one recorded row against the expected
//! original, on the row a native admission records.

use super::*;
use crate::input_authority::tests::input;
use crate::input_state::StoredInputState;

/// The row a native admission of `admitted` records.
fn row(admitted: &Input) -> InputState {
    StoredInputState::accepted_for_test(admitted)
        .expect("admitted row")
        .state
}

fn association(input: &Input) -> &InputAuthorityAssociation {
    input
        .header()
        .authority_association
        .as_ref()
        .expect("governed")
}

fn observed(
    state: &InputState,
    association: &InputAuthorityAssociation,
    expected: &Input,
) -> Result<(), AdmittedOriginalRefusal> {
    own_admission(state, association, expected).map(|_| ())
}

/// The expected original's own admission is observed: exactly one own
/// contributor, with its association, and the same canonical replay
/// identity.
#[test]
fn the_own_admission_of_the_expected_original_is_observed() {
    let expected = input("caller");
    let state = row(&expected);
    let own = own_admission(&state, association(&expected), &expected).expect("its own admission");
    assert_eq!(own.input_id(), expected.id());
    assert!(own.association() == association(&expected));
}

/// A row whose own contributor is missing, duplicated or another input's,
/// or names another association, is not the expected original's admission.
#[test]
fn a_missing_duplicated_or_other_own_contributor_conflicts() {
    let expected = input("caller");
    let conflict = Err(AdmittedOriginalRefusal::Conflict);

    let mut missing = row(&expected);
    missing.authority_contributors.clear();
    assert_eq!(
        observed(&missing, association(&expected), &expected),
        conflict,
        "no own contributor"
    );

    let mut others = row(&expected);
    others.authority_contributors = row(&input("caller")).authority_contributors;
    assert_eq!(
        observed(&others, association(&expected), &expected),
        conflict,
        "only another input's contributor"
    );

    let mut duplicated = row(&expected);
    let own = duplicated.authority_contributors[0].clone();
    duplicated.authority_contributors.push(own);
    assert_eq!(
        observed(&duplicated, association(&expected), &expected),
        conflict,
        "a duplicated own contributor"
    );

    let other = input("another-requester");
    assert!(association(&other) != association(&expected));
    assert_eq!(
        observed(&row(&expected), association(&other), &expected),
        conflict,
        "another association"
    );
}

/// Presence of a prompt witness is not equality: a row without one, or an
/// expected prompt with other content under the same association and key,
/// conflicts.
#[test]
fn the_recorded_prompt_witness_must_equal_the_expected_prompt() {
    let expected = input("caller");
    let conflict = Err(AdmittedOriginalRefusal::Conflict);

    let mut unwitnessed = row(&expected);
    unwitnessed.prompt_replay_identity = None;
    assert_eq!(
        observed(&unwitnessed, association(&expected), &expected),
        conflict,
        "a row without a prompt witness"
    );

    let Input::Prompt(mut changed) = expected.clone() else {
        panic!("prompt fixture")
    };
    changed.content = meerkat_core::types::ContentInput::Text("other content".into());
    let changed = Input::Prompt(changed);
    assert_eq!(
        observed(&row(&expected), association(&changed), &changed),
        conflict,
        "other content under the same association and key"
    );
}
