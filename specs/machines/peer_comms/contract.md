# PeerCommsMachine

_Generated from the Rust machine catalog. Do not edit by hand._

- Version: `2`
- Rust owner: `meerkat-comms` / `generated::peer_comms`

## State
- Phase enum: `Ready`

## Inputs
- `ClassifyExternalEnvelope`(raw_item_id: RawItemId, require_peer_auth: Bool, sender_name_known: Bool, sender_name: String, fallback_sender_name: String, kind: PeerEnvelopeKind, intent: String, lifecycle_peer_present: Bool, lifecycle_peer: String, handling_mode_present: Bool, handling_mode: HandlingMode, silent_intent: Bool, dismiss_message: Bool)
- `ClassifyPlainEvent`(raw_item_id: RawItemId, source_name: String, handling_mode: HandlingMode)

## Effects
- `DropIngress`
- `SetDismissFlag`
- `EnqueueClassifiedEntry`(raw_item_id: RawItemId, class: PeerInputClass, from_peer: Option<String>, lifecycle_peer: Option<String>, normalized_handling_mode: HandlingMode)

## Helpers
- `EffectiveSender`(sender_name_known: Bool, sender_name: String, fallback_sender_name: String) -> `Option<String>`
- `EffectiveLifecyclePeer`(lifecycle_peer_present: Bool, lifecycle_peer: String, sender_name_known: Bool, sender_name: String, fallback_sender_name: String) -> `Option<String>`
- `NormalizedHandlingMode`(handling_mode_present: Bool, handling_mode: HandlingMode) -> `HandlingMode`

## Invariants

## Transitions
### `DropUntrustedExternal`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(require_peer_auth, raw_item_id, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `require_peer_auth_matches`
  - `sender_name_known_matches`
- Emits: `DropIngress`
- To: `Ready`

### `DropAckExternal`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `kind_matches`
- Emits: `DropIngress`
- To: `Ready`

### `DismissExternalMessage`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `dismiss_message_matches`
- Emits: `SetDismissFlag`
- To: `Ready`

### `EnqueueLifecycleAdded`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `intent_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueLifecycleRetired`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `intent_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueLifecycleUnwired`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `intent_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueLifecycleKickoffFailed`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `intent_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueLifecycleKickoffCancelled`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `intent_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueSilentRequest`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `silent_intent_matches`
  - `not_mob_lifecycle_intent`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueActionableRequest`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `silent_intent_matches`
  - `not_mob_lifecycle_intent`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueActionableMessage`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
  - `dismiss_message_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueueResponse`
- From: `Ready`
- On: `ClassifyExternalEnvelope`(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
- Guards:
  - `not_untrusted_external`
  - `kind_matches`
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

### `EnqueuePlainEvent`
- From: `Ready`
- On: `ClassifyPlainEvent`(raw_item_id, source_name, handling_mode)
- Emits: `EnqueueClassifiedEntry`
- To: `Ready`

## Coverage
### Code Anchors
- `meerkat-comms/src/classify.rs` — canonical ingress classification adapter over PeerCommsAuthority
- `meerkat-comms/src/inbox.rs` — classified inbox enqueue/drop/dismiss shell executor
- `meerkat-comms/src/runtime/comms_runtime.rs` — candidate drain projects stored ingress metadata without reclassification
- `meerkat-comms/src/peer_comms_authority.rs` — canonical peer ingress authority

### Scenarios
- `trusted-ingress-classification` — trusted peer envelope is classified and normalized at ingress before enqueue
- `untrusted-drop` — untrusted or invalid peer work is dropped before runtime admission
- `dismiss-at-ingress` — dismiss messages set the dismiss flag without becoming peer input candidates
