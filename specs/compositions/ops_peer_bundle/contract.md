# ops_peer_bundle

_Generated from the Rust composition catalog. Do not edit by hand._

## Machines
- `ops_lifecycle`: `OpsLifecycleMachine` @ actor `ops_plane`

## Routes

## Target Selectors
- `(none)`

## Driver
- `(none)`

## Transaction Plans
- `(none)`

## Scheduler Rules
- `(none)`

## Structural Requirements
- `(none)`

## Behavioral Invariants
- `(none)`

## Coverage
### Code Anchors
- `meerkat-runtime/src/ops_lifecycle.rs` — ops lifecycle shell that handles ExposeOperationPeer effect
- `meerkat-comms/src/runtime/comms_runtime.rs` — add_trusted_peer wiring from ops to peer comms

### Scenarios
- `peer-ready-handoff` — ops-lifecycle PeerReady triggers peer-comms trust establishment
