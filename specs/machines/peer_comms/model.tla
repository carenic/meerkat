---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for PeerCommsMachine.

CONSTANTS BooleanValues, HandlingModeValues, PeerEnvelopeKindValues, RawItemIdValues, StringValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN SeqRemove(Tail(seq), value) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))

VARIABLES phase, model_step_count

vars == << phase, model_step_count >>

NormalizedHandlingMode(handling_mode_present, handling_mode) == (IF handling_mode_present THEN handling_mode ELSE "Queue")
EffectiveSender(sender_name_known, sender_name, fallback_sender_name) == (IF sender_name_known THEN Some(sender_name) ELSE Some(fallback_sender_name))
EffectiveLifecyclePeer(lifecycle_peer_present, lifecycle_peer, sender_name_known, sender_name, fallback_sender_name) == (IF lifecycle_peer_present THEN Some(lifecycle_peer) ELSE EffectiveSender(sender_name_known, sender_name, fallback_sender_name))

Init ==
    /\ phase = "Ready"
    /\ model_step_count = 0

DropUntrustedExternal(require_peer_auth, raw_item_id, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ (require_peer_auth = TRUE)
    /\ (sender_name_known = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


DropAckExternal(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ (kind = "Ack")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


DismissExternalMessage(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Message")
    /\ (dismiss_message = TRUE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueLifecycleAdded(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (intent = "mob.peer_added")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueLifecycleRetired(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (intent = "mob.peer_retired")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueLifecycleUnwired(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (intent = "mob.peer_unwired")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueLifecycleKickoffFailed(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (intent = "mob.kickoff_failed")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueLifecycleKickoffCancelled(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (intent = "mob.kickoff_cancelled")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueSilentRequest(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (silent_intent = TRUE)
    /\ ((intent # "mob.peer_added") /\ (intent # "mob.peer_retired") /\ (intent # "mob.peer_unwired") /\ (intent # "mob.kickoff_failed") /\ (intent # "mob.kickoff_cancelled"))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueActionableRequest(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Request")
    /\ (silent_intent = FALSE)
    /\ ((intent # "mob.peer_added") /\ (intent # "mob.peer_retired") /\ (intent # "mob.peer_unwired") /\ (intent # "mob.kickoff_failed") /\ (intent # "mob.kickoff_cancelled"))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueActionableMessage(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Message")
    /\ (dismiss_message = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueueResponse(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message) ==
    /\ phase = "Ready"
    /\ ~(((require_peer_auth = TRUE) /\ (sender_name_known = FALSE)))
    /\ (kind = "Response")
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


EnqueuePlainEvent(raw_item_id, source_name, handling_mode) ==
    /\ phase = "Ready"
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1


Next ==
    \/ \E require_peer_auth \in BOOLEAN : \E raw_item_id \in RawItemIdValues : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : DropUntrustedExternal(require_peer_auth, raw_item_id, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : DropAckExternal(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : DismissExternalMessage(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueLifecycleAdded(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueLifecycleRetired(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueLifecycleUnwired(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueLifecycleKickoffFailed(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueLifecycleKickoffCancelled(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueSilentRequest(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueActionableRequest(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueActionableMessage(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E require_peer_auth \in BOOLEAN : \E sender_name_known \in BOOLEAN : \E sender_name \in StringValues : \E fallback_sender_name \in StringValues : \E kind \in PeerEnvelopeKindValues : \E intent \in StringValues : \E lifecycle_peer_present \in BOOLEAN : \E lifecycle_peer \in StringValues : \E handling_mode_present \in BOOLEAN : \E handling_mode \in HandlingModeValues : \E silent_intent \in BOOLEAN : \E dismiss_message \in BOOLEAN : EnqueueResponse(raw_item_id, require_peer_auth, sender_name_known, sender_name, fallback_sender_name, kind, intent, lifecycle_peer_present, lifecycle_peer, handling_mode_present, handling_mode, silent_intent, dismiss_message)
    \/ \E raw_item_id \in RawItemIdValues : \E source_name \in StringValues : \E handling_mode \in HandlingModeValues : EnqueuePlainEvent(raw_item_id, source_name, handling_mode)


CiStateConstraint == /\ model_step_count <= 6
DeepStateConstraint == /\ model_step_count <= 8

Spec == Init /\ [][Next]_vars


=============================================================================
