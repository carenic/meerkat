# SessionTurnAdmissionMachine

_Generated from the Rust machine catalog. Do not edit by hand._

- Version: `1`
- Rust owner: `meerkat-session` / `generated::session_turn_admission`

## State
- Phase enum: `Idle | Admitted | Running | Completing | ShuttingDown`
- `interrupt_pending`: `Bool`
- `shutdown_pending`: `Bool`

## Inputs
- `RequestStartTurn`
- `AbortAdmittedTurn`
- `BeginRun`
- `ResolveRun`
- `FinalizeTurn`
- `RequestInterrupt`
- `RequestShutdown`

## Effects
- `WakeInterrupt`

## Invariants
- `interrupt_pending_only_while_active`

## Transitions
### `RequestStartTurn`
- From: `Idle`
- On: `RequestStartTurn`()
- To: `Admitted`

### `AbortAdmittedTurn`
- From: `Admitted`
- On: `AbortAdmittedTurn`()
- To: `Idle`

### `BeginRun`
- From: `Admitted`
- On: `BeginRun`()
- To: `Running`

### `ShutdownFromAdmitted`
- From: `Admitted`
- On: `RequestShutdown`()
- To: `ShuttingDown`

### `ResolveRun`
- From: `Running`
- On: `ResolveRun`()
- To: `Completing`

### `RequestInterrupt`
- From: `Running`
- On: `RequestInterrupt`()
- Emits: `WakeInterrupt`
- To: `Running`

### `RequestShutdownFromRunning`
- From: `Running`
- On: `RequestShutdown`()
- To: `Running`

### `RequestShutdownFromCompleting`
- From: `Completing`
- On: `RequestShutdown`()
- To: `Completing`

### `FinalizeTurnToIdle`
- From: `Completing`
- On: `FinalizeTurn`()
- Guards:
  - `shutdown_pending_false`
- To: `Idle`

### `FinalizeTurnToShuttingDown`
- From: `Completing`
- On: `FinalizeTurn`()
- Guards:
  - `shutdown_pending_true`
- To: `ShuttingDown`

### `RequestShutdownFromIdle`
- From: `Idle`
- On: `RequestShutdown`()
- To: `ShuttingDown`

## Coverage
### Code Anchors
- `meerkat-session/src/ephemeral.rs` — session task shell executing admission authority effects and projections
- `meerkat-session/src/session_turn_admission_authority.rs` — canonical session turn admission authority

### Scenarios
- `admit-begin-resolve-finalize` — a single turn claims the session slot, runs, resolves, and releases admission canonically
- `abort-admitted-pre_run-failure` — pre-run failures after admission release the slot without leaving a busy session behind
- `graceful-shutdown-drain` — shutdown drains running turns and blocks new admissions once shutting down
