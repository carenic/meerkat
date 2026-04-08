# PeerCommsMachine Mapping Note

This note maps the normative `0.5` `PeerCommsMachine` contract onto current
`0.4` anchors.

## Rust anchors

- comms runtime/inbox ownership:
  - `meerkat-comms/src/runtime/comms_runtime.rs`
  - `meerkat-comms/src/inbox.rs`
- current normalization/classification:
  - `meerkat-comms/src/classify.rs`
  - `meerkat-comms/src/agent/types.rs`
- trust store and router:
  - `meerkat-comms/src/trust.rs`
  - `meerkat-comms/src/router.rs`
- current host/runtime bridge:
  - `meerkat-core/src/agent/comms_impl.rs`
  - `meerkat-core/src/agent/runner.rs`

## What is already aligned

- there is already one shared trusted-peer store
- current ingress already snapshots trust/classification before downstream
  conversion
- peer lifecycle notifications are already normalized into explicit classes
- acks are already transport-side and not surfaced into the core agent loop
- runtime-backed peer admission already exists through `RuntimeCommsInputSink`

## What the formal model abstracts

The TLA+ model deliberately abstracts away:

- transport socket/task mechanics
- cryptographic signature payloads
- request/response correlation payloads
- exact inbox channel implementation
- transcript/session mutation
- host-loop-specific batching behavior

Those are implementation details refining the same peer normalization contract.

## Intentional `0.5` shift

The formal contract describes the architectural responsibility, not the legacy
classified-inbox mechanism.

In `0.5`:

- the classified-inbox bypass path should die
- the responsibility survives as admission-time peer normalization
- typed `PeerInput` submission should happen through runtime admission rather
  than through a parallel direct-agent execution path

## Known `0.4` divergence

- classification and queueing are currently coupled to inbox implementation
- host-mode routing still exists outside the canonical runtime path
- `SubagentResult` currently leaks through comms/inbox machinery even though
  `0.5` moves that responsibility into `OpsLifecycleMachine`

<!-- GENERATED_COVERAGE_START -->
## Generated Coverage
This section is generated from the Rust machine catalog. Do not edit it by hand.

### Machine
- `PeerCommsMachine`

### Code Anchors
- `peer_classify`: `meerkat-comms/src/classify.rs` — canonical ingress classification adapter over PeerCommsAuthority
- `peer_inbox`: `meerkat-comms/src/inbox.rs` — classified inbox enqueue/drop/dismiss shell executor
- `peer_runtime`: `meerkat-comms/src/runtime/comms_runtime.rs` — candidate drain projects stored ingress metadata without reclassification
- `peer_authority`: `meerkat-comms/src/peer_comms_authority.rs` — canonical peer ingress authority

### Scenarios
- `trusted-ingress-classification` — trusted peer envelope is classified and normalized at ingress before enqueue
- `untrusted-drop` — untrusted or invalid peer work is dropped before runtime admission
- `dismiss-at-ingress` — dismiss messages set the dismiss flag without becoming peer input candidates

### Transitions
- `DropUntrustedExternal`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `DropAckExternal`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `DismissExternalMessage`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueLifecycleAdded`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueLifecycleRetired`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueLifecycleUnwired`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueLifecycleKickoffFailed`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueLifecycleKickoffCancelled`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueSilentRequest`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueActionableRequest`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueActionableMessage`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueResponse`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueuePlainEvent`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`

### Effects
- `DropIngress`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `SetDismissFlag`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`
- `EnqueueClassifiedEntry`
  - anchors: `peer_classify`, `peer_inbox`, `peer_runtime`, `peer_authority`
  - scenarios: `trusted-ingress-classification`, `untrusted-drop`

### Invariants
- `(none)`


<!-- GENERATED_COVERAGE_END -->
