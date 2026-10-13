//! D50 S2 on a governed member: the host registration hook is shown the
//! member's actual final tool manifest, exactly the tools the member's
//! model is offered on its first governed turn.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

// The shared fixture re-exports what its other users need.
#[allow(unused_imports)]
mod governed_support;

use std::collections::BTreeSet;
use std::sync::Arc;

use governed_support::*;

/// The host sees, before the member is published, exactly the tool names
/// the member's model is then offered when its first governed input runs.
#[tokio::test(flavor = "multi_thread")]
async fn the_host_is_shown_exactly_the_manifest_the_members_model_is_offered() {
    let hook = Arc::new(SurfaceRecordingHook::default());
    let fixture = Fixture::with_options(FixtureOptions {
        registration_hook: Some(Arc::clone(&hook)),
        ..Default::default()
    })
    .await;
    let shown = hook
        .surfaces
        .lock()
        .unwrap()
        .iter()
        .find(|(member, _)| member == MEMBER)
        .map(|(_, tools)| tools.clone())
        .expect("the host was shown the built member's surface");

    let turn = fixture
        .handle
        .start_governed_input(&fixture.member, fixture.bound(|_| {}))
        .await
        .expect("native admission accepts the bound original at the hook-installed destination");
    tokio::time::timeout(WAIT, turn.wait())
        .await
        .expect("the governed turn completes")
        .expect("the governed turn succeeds");
    let offered = fixture
        .provider
        .offered_tools()
        .first()
        .cloned()
        .expect("the member's model was asked once");

    assert!(!shown.is_empty(), "the member has tools to offer");
    assert_eq!(
        shown.iter().cloned().collect::<BTreeSet<_>>(),
        offered.iter().cloned().collect::<BTreeSet<_>>(),
        "the host was shown exactly the final manifest the member's model is offered"
    );
}
