# SessionTurnAdmissionMachine Mapping Note

<!-- GENERATED_COVERAGE_START -->
## Generated Coverage
This section is generated from the Rust machine catalog. Do not edit it by hand.

### Machine
- `SessionTurnAdmissionMachine`

### Code Anchors
- `session_ephemeral`: `meerkat-session/src/ephemeral.rs` — session task shell executing admission authority effects and projections
- `session_turn_admission_authority`: `meerkat-session/src/session_turn_admission_authority.rs` — canonical session turn admission authority

### Scenarios
- `admit-begin-resolve-finalize` — a single turn claims the session slot, runs, resolves, and releases admission canonically
- `abort-admitted-pre_run-failure` — pre-run failures after admission release the slot without leaving a busy session behind
- `graceful-shutdown-drain` — shutdown drains running turns and blocks new admissions once shutting down

### Transitions
- `RequestStartTurn`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `AbortAdmittedTurn`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `BeginRun`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `ShutdownFromAdmitted`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `ResolveRun`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `RequestInterrupt`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `RequestShutdownFromRunning`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `RequestShutdownFromCompleting`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `FinalizeTurnToIdle`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `FinalizeTurnToShuttingDown`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`
- `RequestShutdownFromIdle`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`

### Effects
- `WakeInterrupt`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`

### Invariants
- `interrupt_pending_only_while_active`
  - anchors: `session_ephemeral`, `session_turn_admission_authority`
  - scenarios: `admit-begin-resolve-finalize`


<!-- GENERATED_COVERAGE_END -->
