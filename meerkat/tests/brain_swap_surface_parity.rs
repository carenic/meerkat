//! Every runtime-backed surface must expose the SAME pre-dequeue handle.
//!
//! This is a source-level pin on purpose. The failure it guards against is
//! invisible at runtime: a surface that silently returns `None` from
//! `pre_dequeue_handle` produces sessions where a committed handoff is never
//! realized, and the session simply keeps answering on the old model as if
//! nothing had ever been requested. No test of that surface's own behaviour
//! would notice, because nothing it does is wrong — it just never does the
//! thing.
//!
//! Pinning the shared facade helper by name also prevents the other failure
//! mode: a surface growing its own realization implementation, at which point
//! there are two owners of one cross-run transaction.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::PathBuf;

const SHARED_HELPER: &str = "persistent_runtime_pre_dequeue_handle";
const HOOK_FN: &str = "fn pre_dequeue_handle(";

/// Every production `CoreExecutor` impl that is backed by the shared runtime.
///
/// The RPC crate contributes two: the ordinary session executor and the
/// mob-over-RPC executor. Both are runtime-backed, so both are listed.
const RUNTIME_BACKED_EXECUTOR_SOURCES: &[(&str, &str)] = &[
    ("meerkat/src/surface/runtime_backed.rs", "facade"),
    ("meerkat-cli/src/main.rs", "cli"),
    ("meerkat-rest/src/lib.rs", "rest"),
    ("meerkat-rpc/src/session_executor.rs", "rpc"),
    ("meerkat-mcp-server/src/runtime_ingress.rs", "mcp-server"),
    ("meerkat-mob/src/runtime/provisioner.rs", "mob"),
];

/// Surfaces that deliberately do NOT realize handoffs, and must not pretend to.
const NON_RUNTIME_BACKED_SOURCES: &[(&str, &str)] = &[
    ("meerkat-web-runtime/src/lib.rs", "wasm browser runtime"),
    (
        "meerkat/src/agent_builder.rs",
        "standalone embedded builder",
    ),
];

fn repo_root() -> PathBuf {
    std::env::var_os("MEERKAT_WORKSPACE_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .filter(|root| root.join("Cargo.toml").is_file())
        })
        .expect("test must run from the workspace or through scripts/repo-cargo")
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn every_runtime_backed_surface_exposes_the_pre_dequeue_hook() {
    for (relative, surface) in RUNTIME_BACKED_EXECUTOR_SOURCES {
        let source = read(relative);
        assert!(
            source.contains(HOOK_FN),
            "{surface} ({relative}) does not override `pre_dequeue_handle`; its sessions would \
             silently never realize a committed model-routing handoff"
        );
    }
}

#[test]
fn every_runtime_backed_surface_uses_the_shared_facade_helper() {
    for (relative, surface) in RUNTIME_BACKED_EXECUTOR_SOURCES {
        let source = read(relative);
        assert!(
            source.contains(SHARED_HELPER),
            "{surface} ({relative}) must build its pre-dequeue handle from the shared facade \
             helper `{SHARED_HELPER}`, not a surface-local implementation"
        );
    }
}

/// The facade owns exactly one implementation of the handle.
///
/// If a second `impl CoreExecutorPreDequeueHandle` appears anywhere, the
/// cross-run transaction has acquired a second owner.
#[test]
fn the_pre_dequeue_handle_has_exactly_one_production_implementation() {
    let facade = read("meerkat/src/surface/runtime_backed.rs");
    let implementations = facade
        .matches("impl meerkat_core::lifecycle::CoreExecutorPreDequeueHandle")
        .count()
        + facade.matches("impl CoreExecutorPreDequeueHandle").count();
    assert_eq!(
        implementations, 1,
        "the facade must carry exactly one pre-dequeue handle implementation"
    );

    for (relative, surface) in RUNTIME_BACKED_EXECUTOR_SOURCES
        .iter()
        .filter(|(relative, _)| *relative != "meerkat/src/surface/runtime_backed.rs")
    {
        let source = read(relative);
        assert!(
            !source.contains("impl CoreExecutorPreDequeueHandle")
                && !source.contains("impl meerkat_core::lifecycle::CoreExecutorPreDequeueHandle"),
            "{surface} ({relative}) must not implement the pre-dequeue handle itself"
        );
    }
}

/// Standalone and WASM surfaces neither realize handoffs nor advertise the tool
/// that stages them. Omission here is the correct behaviour, and asserting it
/// keeps a future "just wire it everywhere" change honest: those surfaces have
/// no runtime loop to hook.
#[test]
fn non_runtime_backed_surfaces_omit_the_hook() {
    for (relative, surface) in NON_RUNTIME_BACKED_SOURCES {
        let source = read(relative);
        assert!(
            !source.contains(HOOK_FN),
            "{surface} ({relative}) has no runtime loop and must not advertise a pre-dequeue hook"
        );
        assert!(
            !source.contains(SHARED_HELPER),
            "{surface} ({relative}) must not reach the runtime-backed realization helper"
        );
    }
}

/// The tool is registered only through the composite's gated entry point, so a
/// surface cannot construct it directly and bypass the availability and
/// host-readiness conditions.
#[test]
fn the_builtin_is_only_registered_through_the_gated_entry_point() {
    let mut offenders = Vec::new();
    for (relative, _) in RUNTIME_BACKED_EXECUTOR_SOURCES
        .iter()
        .chain(NON_RUNTIME_BACKED_SOURCES.iter())
    {
        let source = read(relative);
        if source.contains("BrainSwapTool::new") {
            offenders.push(*relative);
        }
    }
    assert!(
        offenders.is_empty(),
        "these surfaces construct the builtin directly, bypassing the availability and \
         host-readiness gate: {offenders:?}"
    );
}
