//! The router's shutdown report: one typed record per close the router owned
//! (C1), the close bound (C3), the narrow settlement attestation (C4), a
//! bounded direct connect that reports its close (C6), and the close index
//! that starts and observes every close without the router lock (C8).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::connection::CustodyNotHeld;
use std::collections::HashMap;

fn surface_router() -> McpRouter {
    McpRouter::new_with_surface_handle(Arc::new(
        meerkat_runtime::RuntimeExternalToolSurfaceHandle::ephemeral(),
    ))
}

#[cfg(unix)]
fn receipt(
    leader: crate::McpStdioLeaderOutcome,
    witness: crate::McpStdioWitnessOutcome,
) -> Arc<crate::McpStdioCloseReceipt> {
    Arc::new(crate::McpStdioCloseReceipt {
        settled_in: None,
        leader_exit: crate::McpStdioLeaderExitObservation::NotSeen,
        witness,
        sigterm: None,
        sigkill: None,
        final_group_signal: None,
        leader,
        elapsed: Duration::ZERO,
    })
}

#[cfg(unix)]
fn reaped() -> crate::McpStdioLeaderOutcome {
    use std::os::unix::process::ExitStatusExt as _;
    crate::McpStdioLeaderOutcome::Reaped(std::process::ExitStatus::from_raw(0))
}

/// The receipt of a stdio close outcome.
fn stdio_receipt(outcome: &McpServerCloseOutcome) -> &crate::McpStdioCloseReceipt {
    match outcome {
        McpServerCloseOutcome::Stdio(receipt) => receipt,
        other => panic!("expected a stdio close: {other:?}"),
    }
}

/// C4: a stdio close settles only on its final facts, a reaped leader and a
/// closed stdout witness; `settled_in` alone never settles it. A close with
/// no process, or of a server never spawned, settles; a failed, not-held or
/// failed-task close never does.
#[cfg(unix)]
#[test]
fn only_a_reaped_leader_with_a_closed_witness_settles_a_stdio_close() {
    use crate::McpStdioClosePhase as Phase;
    use crate::McpStdioLeaderOutcome as Leader;
    use crate::McpStdioWitnessOutcome as Witness;

    for settled in [
        McpServerCloseOutcome::Stdio(receipt(reaped(), Witness::Closed(Phase::Kill))),
        McpServerCloseOutcome::NoProcess,
        McpServerCloseOutcome::NeverSpawned,
    ] {
        assert!(settled.is_settled(), "{settled:?} did not settle");
    }

    let seen_settled_but_reap_failed = crate::McpStdioCloseReceipt {
        settled_in: Some(Phase::Eof),
        leader_exit: crate::McpStdioLeaderExitObservation::Exited(Phase::Eof),
        witness: Witness::Closed(Phase::Eof),
        sigterm: None,
        sigkill: None,
        final_group_signal: None,
        leader: Leader::WaitFailed(std::io::Error::from_raw_os_error(nix::libc::ECHILD)),
        elapsed: Duration::ZERO,
    };
    let unsettled = [
        McpServerCloseOutcome::Stdio(Arc::new(seen_settled_but_reap_failed)),
        McpServerCloseOutcome::Stdio(receipt(Leader::NotReaped, Witness::Closed(Phase::Kill))),
        McpServerCloseOutcome::Stdio(receipt(reaped(), Witness::StillOpen)),
        McpServerCloseOutcome::Stdio(receipt(reaped(), Witness::Absent)),
        McpServerCloseOutcome::Stdio(receipt(
            reaped(),
            Witness::ReadError {
                phase: Phase::Term,
                kind: std::io::ErrorKind::Other,
            },
        )),
        McpServerCloseOutcome::CloseFailed(Arc::new(McpError::ConnectionFailed {
            reason: "client close failed".to_string(),
        })),
        McpServerCloseOutcome::NotHeld(crate::McpCustodyNotHeld::ClosingElsewhere),
        McpServerCloseOutcome::NotHeld(crate::McpCustodyNotHeld::ClosedElsewhere),
        McpServerCloseOutcome::NotHeld(crate::McpCustodyNotHeld::AbandonedElsewhere),
        McpServerCloseOutcome::TaskFailed { panicked: true },
        McpServerCloseOutcome::TaskFailed { panicked: false },
    ];
    for outcome in &unsettled {
        assert!(!outcome.is_settled(), "{outcome:?} settled");
    }
}

/// C1: a close reports `NoProcess` only for a connection with no stdio
/// custody whose client closed, and `NeverSpawned` only for a stdio custody
/// that never held a process; a failed client close with no process is
/// `CloseFailed`. A process close it could not join is `NotHeld`, never
/// either, and a stdio receipt is kept typed (a failed reap included)
/// whatever the client cancel did.
#[cfg(unix)]
#[test]
fn a_close_reports_no_process_only_when_it_held_none() {
    use crate::McpStdioClosePhase as Phase;
    use crate::McpStdioLeaderOutcome as Leader;
    use crate::McpStdioWitnessOutcome as Witness;
    use crate::connection::{ObservedClose, ObservedProcessClose, close_outcome};

    let failure = || McpError::ConnectionFailed {
        reason: "client close failed".to_string(),
    };
    let outcome = |process, cancel_failure| {
        close_outcome(
            "server",
            ObservedClose {
                process,
                cancel_failure,
            },
        )
    };

    let closed = outcome(ObservedProcessClose::NoCustody, None);
    assert!(
        matches!(closed, McpServerCloseOutcome::NoProcess),
        "{closed:?}"
    );
    let closed = outcome(
        ObservedProcessClose::NotHeld(CustodyNotHeld::NeverSpawned),
        None,
    );
    assert!(
        matches!(closed, McpServerCloseOutcome::NeverSpawned),
        "{closed:?}"
    );
    for process in [
        ObservedProcessClose::NoCustody,
        ObservedProcessClose::NotHeld(CustodyNotHeld::NeverSpawned),
    ] {
        let closed = outcome(process, Some(failure()));
        assert!(
            matches!(closed, McpServerCloseOutcome::CloseFailed(_)),
            "{closed:?}"
        );
    }

    for (not_held, expected) in [
        (
            CustodyNotHeld::ClosingElsewhere,
            crate::McpCustodyNotHeld::ClosingElsewhere,
        ),
        (
            CustodyNotHeld::ClosedElsewhere,
            crate::McpCustodyNotHeld::ClosedElsewhere,
        ),
        (
            CustodyNotHeld::AbandonedElsewhere,
            crate::McpCustodyNotHeld::AbandonedElsewhere,
        ),
    ] {
        for cancel_failure in [None, Some(failure())] {
            match outcome(ObservedProcessClose::NotHeld(not_held), cancel_failure) {
                McpServerCloseOutcome::NotHeld(reported) => assert_eq!(reported, expected),
                other => panic!("{not_held:?} reported as {other:?}"),
            }
        }
    }

    // C9: a remote client close that outlasted its bound is a typed failure.
    let closed = outcome(
        ObservedProcessClose::NoCustody,
        Some(McpError::CloseTimedOut {
            server: "remote".to_string(),
            bound: crate::McpStdioCloseBounds::DEFAULT.close_bound(),
        }),
    );
    assert!(
        matches!(&closed, McpServerCloseOutcome::CloseFailed(error)
            if matches!(**error, McpError::CloseTimedOut { .. })),
        "{closed:?}"
    );

    let closed = outcome(
        ObservedProcessClose::Receipt(receipt(reaped(), Witness::Closed(Phase::Eof))),
        Some(failure()),
    );
    assert!(
        matches!(closed, McpServerCloseOutcome::Stdio(_)) && closed.is_settled(),
        "{closed:?}"
    );
    let closed = outcome(
        ObservedProcessClose::Receipt(receipt(
            Leader::WaitFailed(std::io::Error::from_raw_os_error(nix::libc::ECHILD)),
            Witness::Closed(Phase::Eof),
        )),
        None,
    );
    assert!(
        matches!(stdio_receipt(&closed).leader, Leader::WaitFailed(_)),
        "{closed:?}"
    );
}

/// C1: a close task that ends without reporting is still recorded, as
/// `TaskFailed`, against the server it was closing.
#[tokio::test]
async fn a_close_task_that_panics_is_reported_against_its_server() {
    let mut router = surface_router();
    let task = router
        .closing
        .spawn(async { std::panic::panic_any("a close task that ends without reporting") });
    router
        .closing_servers
        .insert(task.id(), "panicking".to_string());

    let report = router.shutdown_with_report().await;

    match report.servers() {
        [
            McpServerCloseRecord {
                server,
                outcome: McpServerCloseOutcome::TaskFailed { panicked: true },
            },
        ] => assert_eq!(server, "panicking"),
        other => panic!("{other:?}"),
    }
    assert!(!report.is_settled());
}

/// C1: a host test builds a report from exact records.
#[test]
fn a_report_from_records_settles_only_when_every_record_does() {
    let settled = McpRouterShutdownReport::from_records(vec![McpServerCloseRecord {
        server: "remote".to_string(),
        outcome: McpServerCloseOutcome::NoProcess,
    }]);
    assert_eq!(settled.servers().len(), 1);
    assert!(settled.is_settled());
    let unsettled = McpRouterShutdownReport::from_records(vec![
        McpServerCloseRecord {
            server: "remote".to_string(),
            outcome: McpServerCloseOutcome::NoProcess,
        },
        McpServerCloseRecord {
            server: "stdio".to_string(),
            outcome: McpServerCloseOutcome::TaskFailed { panicked: false },
        },
    ]);
    assert!(!unsettled.is_settled());
}

/// C3: the close bound is both graces plus the fixed backstop after the
/// kill, usable in a constant.
#[test]
fn close_bound_is_both_graces_plus_the_kill_backstop() {
    const DEFAULT_BOUND: Duration = crate::McpStdioCloseBounds::DEFAULT.close_bound();
    let backstop = Duration::from_secs(10);
    assert_eq!(DEFAULT_BOUND, Duration::from_secs(5 + 3) + backstop);
    assert_eq!(
        crate::McpStdioCloseBounds::IMMEDIATE.close_bound(),
        backstop
    );
    let max = crate::McpStdioCloseBounds::new(
        crate::McpStdioCloseBounds::MAX_GRACE,
        crate::McpStdioCloseBounds::MAX_GRACE,
    )
    .expect("bounds within the maximum");
    assert_eq!(
        max.close_bound(),
        crate::McpStdioCloseBounds::MAX_GRACE * 2 + backstop
    );
}

#[cfg(target_os = "linux")]
mod stdio {
    use super::*;
    use crate::stdio_test_fixture::{
        PidReport, process_exited, sh_mcp_server_args, sh_mcp_server_args_with_prelude,
        sh_mcp_server_script,
    };

    /// A shell MCP server that exits as soon as its stdin closes.
    fn exits_on_eof(name: &str) -> McpServerConfig {
        McpServerConfig::stdio(
            name,
            "/bin/sh",
            sh_mcp_server_script("", ""),
            HashMap::new(),
        )
    }

    /// A shell MCP server that ignores stdin EOF and first starts a process
    /// that escapes its process group (`setsid`) holding its stdout; the
    /// escaped process writes its own pid to `report` once it has escaped.
    fn escaped_stdout_holder(name: &str, report: &PidReport) -> McpServerConfig {
        McpServerConfig::stdio(
            name,
            "/bin/sh",
            sh_mcp_server_args_with_prelude(&format!(
                "setsid sh -c 'echo $$ > \"{}\"; exec sleep 60' &\n",
                report.path().display()
            )),
            HashMap::new(),
        )
    }

    /// A shell process that never answers its handshake, ignores stdin EOF
    /// and writes its pid to `report`.
    fn never_answers(name: &str, report: &PidReport) -> McpServerConfig {
        McpServerConfig::stdio(
            name,
            "/bin/sh",
            vec![
                "-c".to_string(),
                format!("echo $$ > '{}'; exec sleep 60", report.path().display()),
            ],
            HashMap::new(),
        )
    }

    /// Wait until the router reports `server`'s connect attempt failed: a
    /// refused attempt fails at once.
    async fn attempt_failed(adapter: &crate::McpRouterAdapter, server: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actions = adapter
                    .poll_lifecycle_actions()
                    .await
                    .expect("lifecycle actions");
                if actions.iter().any(|action| {
                    action.target == server
                        && matches!(action.phase, crate::McpLifecyclePhase::Failed)
                }) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the attempt was refused at once");
    }

    fn kill(pid: u32) {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(pid).expect("pid fits i32"));
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    }

    fn only_record(report: &McpRouterShutdownReport) -> &McpServerCloseRecord {
        match report.servers() {
            [record] => record,
            other => panic!("expected one close record: {other:?}"),
        }
    }

    fn assert_reaped_with(
        record: &McpServerCloseRecord,
        witness: impl Fn(&crate::McpStdioWitnessOutcome) -> bool,
    ) {
        let receipt = stdio_receipt(&record.outcome);
        assert!(
            matches!(receipt.leader, crate::McpStdioLeaderOutcome::Reaped(_)),
            "{record:?}"
        );
        assert!(witness(&receipt.witness), "{record:?}");
    }

    fn closed(witness: &crate::McpStdioWitnessOutcome) -> bool {
        matches!(witness, crate::McpStdioWitnessOutcome::Closed(_))
    }

    fn still_open(witness: &crate::McpStdioWitnessOutcome) -> bool {
        matches!(witness, crate::McpStdioWitnessOutcome::StillOpen)
    }

    /// C1: shutdown reports an established stdio server's close with its
    /// receipt, and a server that exits on EOF settles (C4). (Fails at base:
    /// the receipt was only traced, and shutdown reported nothing.)
    #[tokio::test]
    async fn shutdown_reports_an_established_servers_settled_close() {
        let mut router = surface_router();
        router
            .add_server(exits_on_eof("cooperative"))
            .await
            .expect("add_server");

        let report = router.shutdown_with_report().await;

        let record = only_record(&report);
        assert_eq!(record.server, "cooperative");
        assert_reaped_with(record, closed);
        assert!(report.is_settled());
    }

    /// C1: shutdown reports the close of a connect attempt it aborted
    /// mid-handshake, through the router's custody of the attempt's process.
    /// (Fails at base: that receipt was traced and dropped.)
    #[tokio::test]
    async fn shutdown_reports_a_still_connecting_servers_close() {
        let mut pending = PidReport::new("report-still-connecting");
        let mut router =
            surface_router().with_stdio_close_bounds(crate::McpStdioCloseBounds::IMMEDIATE);
        router
            .stage_add(never_answers("still-connecting", &pending))
            .expect("stage add");
        router.apply_staged().await.expect("apply staged add");
        let pids = pending.pids().await;

        let report = router.shutdown_with_report().await;

        let record = only_record(&report);
        assert_eq!(record.server, "still-connecting");
        assert_reaped_with(record, closed);
        assert!(report.is_settled());
        for pid in pids {
            assert!(process_exited(pid), "stdio process {pid} outlived shutdown");
        }
    }

    /// C4, the noncooperative case: a server whose stdout escaped its process
    /// group is killed and reaped, but its stdout witness is still open when
    /// the backstop expires. The report keeps the typed receipt and does not
    /// settle. The test kills the escaped process at its end.
    #[tokio::test]
    async fn an_escaped_stdout_holder_leaves_the_close_unsettled() {
        let mut escaped = PidReport::new("report-escaped-holder");
        let mut router =
            surface_router().with_stdio_close_bounds(crate::McpStdioCloseBounds::IMMEDIATE);
        let config = escaped_stdout_holder("escaped", &escaped);
        let (added, holder) = tokio::join!(router.add_server(config), escaped.pids());
        added.expect("add_server");
        let [holder] = holder[..] else {
            panic!("one escaped pid: {holder:?}");
        };

        let report = router.shutdown_with_report().await;
        kill(holder);

        let record = only_record(&report);
        assert_eq!(record.server, "escaped");
        assert_reaped_with(record, still_open);
        assert!(!report.is_settled());
    }

    /// C1: a removal close that ended unsettled before shutdown is kept, once
    /// reaped, and still reported by the shutdown.
    #[tokio::test]
    async fn an_unsettled_removal_close_is_kept_for_the_report() {
        let mut escaped = PidReport::new("report-removed-holder");
        let mut router =
            surface_router().with_stdio_close_bounds(crate::McpStdioCloseBounds::IMMEDIATE);
        let config = escaped_stdout_holder("removed", &escaped);
        let (added, holder) = tokio::join!(router.add_server(config), escaped.pids());
        added.expect("add_server");
        let [holder] = holder[..] else {
            panic!("one escaped pid: {holder:?}");
        };
        router.stage_remove("removed").expect("stage remove");
        router.apply_staged().await.expect("apply remove");
        // The removal close runs in its own task; reap it once it has ended
        // (after the backstop, since the escaped process holds the stdout).
        let reaped = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                router.reap_closing();
                if router.closing.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        kill(holder);
        reaped.expect("the removal close ends within its bounds");
        assert_eq!(
            router.unsettled_closes.len(),
            1,
            "the unsettled close is kept"
        );

        let report = router.shutdown_with_report().await;

        let record = only_record(&report);
        assert_eq!(record.server, "removed");
        assert_reaped_with(record, still_open);
        assert!(!report.is_settled());
    }

    /// C1: a failed connect attempt closes its own process, and its custody
    /// keeps the receipt for the router; one that ended unsettled is kept,
    /// once the attempt's result is processed, and reported by the shutdown.
    /// (Fails at base: the attempt traced its receipt and dropped it.)
    #[tokio::test]
    async fn a_failed_attempts_unsettled_close_is_kept_for_the_report() {
        let mut escaped = PidReport::new("report-failed-attempt-holder");
        let mut router =
            surface_router().with_stdio_close_bounds(crate::McpStdioCloseBounds::IMMEDIATE);
        // Never answers its handshake, so its one-second connect fails.
        let mut config = McpServerConfig::stdio(
            "never-answers",
            "/bin/sh",
            vec![
                "-c".to_string(),
                format!(
                    "setsid sh -c 'echo $$ > \"{}\"; exec sleep 60' &\nexec sleep 60\n",
                    escaped.path().display()
                ),
            ],
            HashMap::new(),
        );
        config.connect_timeout_secs = Some(1);
        router.stage_add(config).expect("stage add");
        router.apply_staged().await.expect("apply staged add");
        let [holder] = escaped.pids().await[..] else {
            panic!("one escaped pid");
        };
        // The attempt's own close ends after the backstop (the escaped
        // process holds the stdout); its result then carries the failure.
        let processed = tokio::time::timeout(Duration::from_secs(30), async {
            while router.unsettled_closes.is_empty() {
                router.drain_pending();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        kill(holder);
        processed.expect("the failed attempt's close was kept");

        let report = router.shutdown_with_report().await;

        let record = only_record(&report);
        assert_eq!(record.server, "never-answers");
        assert_reaped_with(record, still_open);
        assert!(!report.is_settled());
    }

    /// C1: a connection's own close reports its typed outcome; a server that
    /// exits on EOF settles.
    #[tokio::test]
    async fn a_connection_close_reports_its_outcome() {
        let connection = McpConnection::connect(&exits_on_eof("direct"))
            .await
            .expect("connect");
        let outcome = connection.close_with_outcome().await;
        assert!(
            matches!(outcome, McpServerCloseOutcome::Stdio(_)) && outcome.is_settled(),
            "{outcome:?}"
        );
    }

    /// C6/T7: a bounded connect that times out closes the process it spawned
    /// through its custody before returning, and reports that close's final
    /// native facts: the server ignores EOF, so it was terminated, reaped,
    /// and every holder of its stdout closed it.
    #[tokio::test]
    async fn a_timed_out_bounded_connect_reports_its_process_close() {
        let mut started = PidReport::new("connect-within-timeout");
        let config = never_answers("never-answers", &started);
        let (connected, pids) = tokio::join!(
            McpConnection::connect_within(&config, Duration::from_millis(500)),
            started.pids(),
        );
        let Err(failure) = connected else {
            panic!("a server that never answers does not connect");
        };
        assert!(failure.timed_out, "{failure:?}");
        let close = failure.close.as_ref().expect("a spawned process is closed");
        let receipt = stdio_receipt(close);
        assert!(
            matches!(receipt.leader, crate::McpStdioLeaderOutcome::Reaped(_)),
            "{failure:?}"
        );
        assert_eq!(
            receipt.witness,
            crate::McpStdioWitnessOutcome::Closed(crate::McpStdioClosePhase::Term),
            "{failure:?}"
        );
        assert_eq!(
            receipt.sigterm,
            Some(crate::McpStdioSignalOutcome::Sent),
            "{failure:?}"
        );
        assert!(close.is_settled(), "{failure:?}");
        for pid in pids {
            assert!(
                process_exited(pid),
                "stdio process {pid} outlived the failed connect"
            );
        }
    }

    /// C6/T7, the noncooperative case: a bounded connect that times out on
    /// a server whose stdout escaped its process group reports that process
    /// close as the stdio receipt it is, reaped with its stdout witness still
    /// open, never as a close failure. The test kills the escaped process at
    /// its end.
    #[tokio::test]
    async fn a_timed_out_bounded_connect_reports_an_escaped_stdout_holder_as_open() {
        let mut escaped = PidReport::new("connect-within-escaped");
        let config = McpServerConfig::stdio(
            "escaping",
            "/bin/sh",
            vec![
                "-c".to_string(),
                format!(
                    "setsid sh -c 'echo $$ > \"{}\"; exec sleep 60' &\nexec sleep 60\n",
                    escaped.path().display()
                ),
            ],
            HashMap::new(),
        );
        let (connected, holder) = tokio::join!(
            McpConnection::connect_within(&config, Duration::from_millis(500)),
            escaped.pids(),
        );
        let [holder] = holder[..] else {
            panic!("one escaped pid: {holder:?}");
        };
        kill(holder);
        let Err(failure) = connected else {
            panic!("a server that never answers does not connect");
        };
        assert!(failure.timed_out, "{failure:?}");
        let close = failure.close.as_ref().expect("a spawned process is closed");
        let receipt = stdio_receipt(close);
        assert!(
            matches!(receipt.leader, crate::McpStdioLeaderOutcome::Reaped(_)),
            "{failure:?}"
        );
        assert!(still_open(&receipt.witness), "{failure:?}");
        assert!(!close.is_settled(), "{failure:?}");
    }

    /// C6: a bounded connect that fails before spawning anything reports no
    /// close and no timeout.
    #[tokio::test]
    async fn a_bounded_connect_that_spawned_nothing_reports_no_close() {
        let config = McpServerConfig::stdio(
            "missing",
            "/nonexistent/meerkat-mcp-test-server",
            Vec::new(),
            HashMap::new(),
        );
        let Err(failure) = McpConnection::connect_within(&config, Duration::from_secs(10)).await
        else {
            panic!("a missing command does not connect");
        };
        assert!(!failure.timed_out, "{failure:?}");
        assert!(failure.close.is_none(), "{failure:?}");
    }

    /// C8: the adapter starts a server's close without the router lock, while
    /// a dispatch still holds the router's read guard, and refuses new
    /// staging from then on; the shutdown later joins the same close and
    /// reports the same receipt. (Fails at base: the adapter started a close
    /// only after it took the router's write lock.)
    #[tokio::test]
    async fn begin_close_closes_a_server_while_a_dispatch_holds_the_router() {
        let mut router = surface_router();
        router
            .add_server(exits_on_eof("cooperative"))
            .await
            .expect("add_server");
        let adapter = crate::McpRouterAdapter::new(router);
        let held = adapter.router_lock().read().await;

        let started = tokio::time::Instant::now();
        let ticket = adapter.begin_close();
        let deadline = ticket.deadline();
        assert!(
            deadline
                <= started
                    + crate::McpStdioCloseBounds::DEFAULT.close_bound()
                    + crate::McpCloseTicket::JOIN_ALLOWANCE
                    + Duration::from_secs(1),
            "the deadline is the close's own bound"
        );
        let records = tokio::time::timeout_at(deadline, ticket.outcomes())
            .await
            .expect("the close reports by its deadline while the guard is held");
        let [record] = &records[..] else {
            panic!("one close record: {records:?}");
        };
        assert_eq!(record.server, "cooperative");
        assert!(record.outcome.is_settled(), "{record:?}");
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                adapter.stage_add(exits_on_eof("late"))
            )
            .await
            .expect("refused before taking the router guard")
            .expect_err("staging is refused once the adapter is closing"),
            "MCP router is closing"
        );
        drop(held);

        let crate::McpRouterShutdownObservation::Reported(report) =
            adapter.shutdown_with_report().await
        else {
            panic!("the shutdown reported");
        };
        let shut = only_record(&report);
        let (McpServerCloseOutcome::Stdio(early), McpServerCloseOutcome::Stdio(late)) =
            (&record.outcome, &shut.outcome)
        else {
            panic!("stdio closes: {record:?} {shut:?}");
        };
        assert!(
            Arc::ptr_eq(early, late),
            "the shutdown joined the same close"
        );
    }

    /// C8: a ticket's deadline counts from when each close actually began. A
    /// removal's close that began before the ticket keeps its own deadline,
    /// earlier than one counted from the ticket, and reports within it.
    #[tokio::test]
    async fn a_ticket_deadline_counts_from_each_closes_own_start() {
        let mut router = surface_router();
        router
            .add_server(McpServerConfig::stdio(
                "eof-ignoring",
                "/bin/sh",
                sh_mcp_server_args(None),
                HashMap::new(),
            ))
            .await
            .expect("add_server");
        router.stage_remove("eof-ignoring").expect("stage remove");
        router.apply_staged().await.expect("apply remove");
        let adapter = crate::McpRouterAdapter::new(router);
        tokio::time::sleep(Duration::from_secs(1)).await;

        let began = tokio::time::Instant::now();
        let ticket = adapter.begin_close();
        assert!(
            ticket.deadline()
                < began
                    + crate::McpStdioCloseBounds::DEFAULT.close_bound()
                    + crate::McpCloseTicket::JOIN_ALLOWANCE,
            "the removal's close began before the ticket, and its deadline counts from then"
        );
        let records = tokio::time::timeout_at(ticket.deadline(), ticket.outcomes())
            .await
            .expect("the close reports by its own deadline");
        let [record] = &records[..] else {
            panic!("one close record: {records:?}");
        };
        assert_eq!(record.server, "eof-ignoring");
        assert_reaped_with(record, closed);
    }

    /// C8, the admission boundary: an attempt admitted before the close
    /// began whose process has not spawned is retired by the close, so it
    /// spawns nothing and reports `NeverSpawned`; an attempt admitted after
    /// the close is refused before any effect and is never indexed.
    #[tokio::test]
    async fn begin_close_retires_an_attempt_that_has_not_spawned() {
        let index = crate::connection::CloseIndex::default();
        let early = index.admit("not-yet", Some(crate::McpStdioCloseBounds::DEFAULT));
        let ticket = index.begin_close();
        let late = index.admit("late", Some(crate::McpStdioCloseBounds::DEFAULT));
        let mut started = PidReport::new("retired-custody");
        let config = never_answers("not-yet", &started);
        for attempt in [early, late] {
            // Bounded: an attempt that did spawn would never connect.
            let connected = tokio::time::timeout(
                Duration::from_secs(10),
                McpConnection::connect_with_custody(
                    &config,
                    McpAuthMode::Stored,
                    None,
                    None,
                    attempt.process.clone(),
                    attempt.client.clone(),
                    &crate::McpStdioLaunchProfile::trusted_host(),
                    &crate::HttpEgressProfile::trusted_host(),
                    crate::connection::PhysicalEntry::default(),
                    None,
                ),
            )
            .await
            .expect("a closed attempt is refused at once");
            assert!(
                matches!(connected, Err(McpError::RouterShutDown)),
                "a closed attempt spawns nothing: {:?}",
                connected.err()
            );
        }
        let records = ticket.outcomes().await;
        assert!(
            matches!(
                &records[..],
                [McpServerCloseRecord { server, outcome: McpServerCloseOutcome::NeverSpawned }]
                    if server == "not-yet"
            ),
            "{records:?}"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), started.line())
                .await
                .is_err(),
            "no process started"
        );
    }

    /// C8, the admission boundary through the router: a staged add admitted
    /// before the close began, but applied after it (it waited for the
    /// router lock a dispatch held), gets its attempt's custodies created
    /// closed under the index's freeze, so nothing spawns, and the shutdown
    /// reports nothing unsettled.
    #[tokio::test]
    async fn an_add_admitted_before_the_close_spawns_nothing_after_it() {
        let mut started = PidReport::new("admitted-before-close");
        let adapter = crate::McpRouterAdapter::new(surface_router());
        adapter
            .stage_add(never_answers("late", &started))
            .await
            .expect("staged before the close");
        let held = adapter.router_lock().read().await;
        let mut apply = Box::pin(adapter.apply_staged());
        assert!(
            futures::poll!(apply.as_mut()).is_pending(),
            "the apply passed admission and waits for the router lock"
        );
        let ticket = adapter.begin_close();
        drop(held);
        apply.await.expect("the admitted apply finishes");
        // The attempt has ended before its effects are checked.
        attempt_failed(&adapter, "late").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(200), started.line())
                .await
                .is_err(),
            "no process started"
        );
        assert!(
            ticket.outcomes().await.is_empty(),
            "nothing was admitted before the close"
        );
        let crate::McpRouterShutdownObservation::Reported(report) =
            adapter.shutdown_with_report().await
        else {
            panic!("the shutdown reported");
        };
        assert!(
            report
                .servers()
                .iter()
                .all(|record| matches!(record.outcome, McpServerCloseOutcome::NeverSpawned)),
            "{report:?}"
        );
    }
}
