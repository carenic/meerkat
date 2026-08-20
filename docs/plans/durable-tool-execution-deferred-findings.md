---
title: "Durable Tool Execution Deferred Findings"
description: "Earlier deferred findings from the durable shell and job execution delivery slices."
icon: "list-check"
---

# Durable Tool Execution Deferred Findings

<Note>
  This is an earlier maintainer implementation ledger, retained as a review
  record rather than current product guidance. See
  [Durable jobs](/guides/durable-jobs) for the shipped contract.
</Note>

This historical ledger was re-audited against `8138813cb` on 2026-08-20.
Entries still do not violate the shipped durable-jobs contract. Recovery and
lifecycle correctness are not deferred here.

| Slice found | Finding | Disposition |
| --- | --- | --- |
| Phase 3A | SQLite origin lookup filters `origin_session_id` after decoding realm rows because the v1 table has no dedicated session column. | Defer the indexed schema optimization to a later jobs-store migration; current results are complete, but the query decodes realm rows before filtering by origin session. |
| Phase 3A | A crash after job submission but before the caller receives the receipt leaves a queued shell job dependent on idempotent call replay; there is no general orphan-queue worker. | Defer a general queue consumer to the multi-runner worker surface. Phase 3A proves replay claims the same committed job exactly once. |
| Phase 3A | Raw shell commands are durable runner configuration and may contain user-inlined credentials. | Typed callback credential references and execution-time resolution shipped. Background shell also rejects configured resolved environment values in the command, but arbitrary user-inlined secret text cannot be identified reliably. Prefer environment-variable references and do not claim arbitrary raw shell commands are secret-safe. |
| Phase 3A gate | `meerkat-mob::runtime::tests::test_shutdown_does_not_stall_on_stuck_lifecycle_notification` transiently observed an unrelated unregister teardown still in progress during the full workspace run, then passed immediately in isolation. | Record as a pre-existing Mob teardown flake; do not expand the durable-shell slice. |

The SQLite origin lookup still filters decoded realm rows because the released
table indexes realm and submission identity, not origin session. A general
worker recovers replayable/checkpoint-resumable monitors and due callback jobs,
but a queued non-resumable shell submission at the commit-before-receipt seam
still relies on replay of its stable tool-call key.
