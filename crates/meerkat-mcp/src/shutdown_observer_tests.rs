//! The adapter's router shutdown observer (C2): one report per adapter,
//! published to observers that outlive it, or Abandoned.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::collections::HashMap;

fn surface_router() -> McpRouter {
    McpRouter::new_with_surface_handle_and_stdio_profile(
        Arc::new(meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral()),
        crate::McpStdioLaunchProfile::trusted_host(),
    )
}

/// C2: the observer and what it reads can be held anywhere, past the adapter.
#[test]
fn the_observer_is_clone_send_sync_static() {
    fn assert_clone_send_sync<T: Clone + Send + Sync + 'static>() {}
    assert_clone_send_sync::<McpRouterShutdownObserver>();
    assert_clone_send_sync::<McpRouterShutdownObservation>();
}

/// C2: an adapter dropped without shutting its router down leaves its
/// observer reading Abandoned. (Fails at base: nothing outside the adapter
/// could observe its router's shutdown.)
#[tokio::test]
async fn an_observer_reads_abandoned_when_the_adapter_is_dropped_unshut() {
    let adapter = McpRouterAdapter::new(surface_router());
    let mut observer = adapter.shutdown_observer();
    drop(adapter);
    let observed = tokio::time::timeout(std::time::Duration::from_secs(5), observer.observe())
        .await
        .expect("the observer reads the dropped adapter's shutdown");
    assert!(
        matches!(observed, McpRouterShutdownObservation::Abandoned),
        "{observed:?}"
    );
}

/// C2: reading without waiting gives nothing until the shutdown reports or
/// is abandoned, then the same observation a wait gives.
#[tokio::test]
async fn current_reads_nothing_until_the_shutdown_reports() {
    let adapter = McpRouterAdapter::new(surface_router());
    let observer = adapter.shutdown_observer();
    assert!(observer.current().is_none(), "nothing reported yet");
    let McpRouterShutdownObservation::Reported(report) = adapter.shutdown_with_report().await
    else {
        panic!("the shutdown reported");
    };
    match observer.current() {
        Some(McpRouterShutdownObservation::Reported(current)) => {
            assert!(Arc::ptr_eq(&current, &report), "the one report");
        }
        other => panic!("{other:?}"),
    }
    assert!(report.servers().is_empty() && report.is_settled());

    let unshut = McpRouterAdapter::new(surface_router());
    let observer = unshut.shutdown_observer();
    drop(unshut);
    assert!(
        matches!(
            observer.current(),
            Some(McpRouterShutdownObservation::Abandoned)
        ),
        "a dropped adapter's shutdown is abandoned"
    );
}

#[cfg(target_os = "linux")]
mod stdio {
    use super::*;
    use crate::stdio_test_fixture::{
        PidReport, exits_within, sh_mcp_server_args, sh_mcp_server_script,
    };

    /// C2: every shutdown call and every observer, taken before the shutdown
    /// or after it and read after the adapter is gone, reads the one report.
    #[tokio::test]
    async fn every_shutdown_and_observer_reads_the_one_report() {
        let mut router = surface_router();
        router
            .add_server(McpServerConfig::stdio(
                "cooperative",
                "/bin/sh",
                sh_mcp_server_script("", ""),
                HashMap::new(),
            ))
            .await
            .expect("add_server");
        let adapter = McpRouterAdapter::new(router);
        let mut early = adapter.shutdown_observer();

        let McpRouterShutdownObservation::Reported(first) = adapter.shutdown_with_report().await
        else {
            panic!("the first shutdown reported");
        };
        let McpRouterShutdownObservation::Reported(second) = adapter.shutdown_with_report().await
        else {
            panic!("the repeated shutdown reported");
        };
        assert!(Arc::ptr_eq(&first, &second), "one shutdown, one report");
        let mut late = adapter.shutdown_observer();
        drop(adapter);

        for observer in [&mut early, &mut late] {
            match observer.observe().await {
                McpRouterShutdownObservation::Reported(report) => {
                    assert!(
                        Arc::ptr_eq(&report, &first),
                        "observers read the one report"
                    );
                }
                McpRouterShutdownObservation::Abandoned => panic!("the shutdown reported"),
            }
        }
        match first.servers() {
            [record] => assert_eq!(record.server, "cooperative"),
            other => panic!("expected one close record: {other:?}"),
        }
        assert!(first.is_settled());
    }

    /// C2: a shutdown task dropped before it reports (its runtime shut down
    /// while the server was in its EOF grace) leaves the observer reading
    /// Abandoned, never a report. (Fails at base: no observer existed; the
    /// adapter only logged that the task ended without finishing.)
    #[test]
    fn an_observer_reads_abandoned_when_the_shutdown_task_is_dropped() {
        let host = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("host runtime");
        let (adapter, observer, pids) = host.block_on(async {
            let mut pid_report = PidReport::new("abandoned-shutdown");
            let bounds = crate::McpStdioCloseBounds::new(
                std::time::Duration::from_secs(20),
                std::time::Duration::from_secs(10),
            )
            .expect("bounds within the maximum");
            let mut router = surface_router().with_stdio_close_bounds(bounds);
            let config = McpServerConfig::stdio(
                "eof-ignoring",
                "/bin/sh",
                sh_mcp_server_args(Some(pid_report.path())),
                HashMap::new(),
            );
            let (added, pids) = tokio::join!(router.add_server(config), pid_report.pids());
            added.expect("add_server");
            let adapter = McpRouterAdapter::new(router);
            let observer = adapter.shutdown_observer();
            let waited = tokio::time::timeout(
                std::time::Duration::from_millis(200),
                adapter.shutdown_with_report(),
            )
            .await;
            assert!(waited.is_err(), "the server was still in its EOF grace");
            (adapter, observer, pids)
        });
        // The runtime shuts down: the shutdown task is dropped before it
        // reports, and the server's close with it (its group is killed).
        drop(host);

        let observe = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("observing runtime");
        let mut observer = observer;
        let observed = observe
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), observer.observe()).await
            })
            .expect("the observer reads the dropped shutdown task");
        assert!(
            matches!(observed, McpRouterShutdownObservation::Abandoned),
            "{observed:?}"
        );
        drop(adapter);
        for pid in pids {
            assert!(
                observe.block_on(exits_within(pid, std::time::Duration::from_secs(5))),
                "stdio process {pid} outlived the dropped shutdown"
            );
        }
    }
}
