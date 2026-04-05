# OccurrenceLifecycleMachine Mapping Note

<!-- GENERATED_COVERAGE_START -->
## Generated Coverage
This section is generated from the Rust machine catalog. Do not edit it by hand.

### Machine
- `OccurrenceLifecycleMachine`

### Code Anchors
- `occurrence_authority`: `meerkat-schedule/src/authority.rs` — occurrence lifecycle authority that owns claim, dispatch, lease expiry, and terminal outcomes
- `schedule_driver`: `meerkat-schedule/src/driver.rs` — mechanical scheduler driver precursor for due claims, probes, dispatch, and feedback
- `schedule_store`: `meerkat-schedule/src/store.rs` — durable claim-time and occurrence state precursor
- `occurrence_schema`: `meerkat-machine-schema/src/catalog/occurrence_lifecycle.rs` — formal OccurrenceLifecycleMachine schema

### Scenarios
- `occurrence-claim-dispatch-complete` — occurrences claim, dispatch, and reach a terminal outcome with attempt ownership preserved
- `occurrence-supersede` — pending occurrences supersede when a newer schedule revision invalidates them
- `occurrence-lease-expiry` — live claimed work returns to pending when a lease expires before completion

### Transitions
- `ClaimPending`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `DispatchStartedFromClaimed`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `AwaitCompletionFromDispatching`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `CompleteFromDispatchingOrAwaiting`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `SkipFromLive`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `MisfireFromLive`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `SupersedePending`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `DeliveryFailedFromLive`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `LeaseExpiredFromClaimed`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `LeaseExpiredFromDispatching`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `LeaseExpiredFromAwaitingCompletion`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`

### Effects
- `Claimed`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `DispatchStarted`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `AwaitingCompletion`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `Completed`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `Skipped`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `Misfired`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `Superseded`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `DeliveryFailed`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `LeaseExpired`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`

### Invariants
- `live_claim_requires_owner`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `superseded_records_revision`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`
- `delivery_failed_records_failure_class`
  - anchors: `occurrence_authority`, `schedule_driver`, `schedule_store`, `occurrence_schema`
  - scenarios: `occurrence-claim-dispatch-complete`


<!-- GENERATED_COVERAGE_END -->
