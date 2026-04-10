---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated composition model for peer_runtime_bundle.

CONSTANTS AdmissionEffectValues, BooleanValues, ContentShapeValues, HandlingModeValues, NatValues, PeerEnvelopeKindValues, PolicyDecisionValues, RawItemIdValues, RequestIdValues, ReservationKeyValues, RunIdValues, SetOfStringValues, StringValues, WorkIdValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

SeqOfWorkIdValues == {<<>>} \cup {<<x>> : x \in WorkIdValues} \cup {<<x, y>> : x \in WorkIdValues, y \in WorkIdValues}
OptionRequestIdValues == {None} \cup {Some(x) : x \in RequestIdValues}
OptionReservationKeyValues == {None} \cup {Some(x) : x \in ReservationKeyValues}

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN Tail(seq) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))
AppendIfMissing(seq, value) == IF value \in SeqElements(seq) THEN seq ELSE Append(seq, value)
Machines == {
    <<"peer_comms", "PeerCommsMachine", "peer_plane">>,
    <<"runtime_control", "RuntimeControlMachine", "control_plane">>,
    <<"runtime_ingress", "RuntimeIngressMachine", "ordinary_ingress">>
}

RouteNames == {
    "peer_candidate_enters_runtime_admission",
    "admitted_peer_work_enters_ingress",
    "peer_ingress_ready_begins_run"
}

Actors == {
    "peer_plane",
    "control_plane",
    "ordinary_ingress"
}

ActorPriorities == {
    <<"control_plane", "peer_plane">>
}

SchedulerRules == {
    <<"PreemptWhenReady", "control_plane", "peer_plane">>
}

ActorOfMachine(machine_id) ==
    CASE machine_id = "peer_comms" -> "peer_plane"
      [] machine_id = "runtime_control" -> "control_plane"
      [] machine_id = "runtime_ingress" -> "ordinary_ingress"
      [] OTHER -> "unknown_actor"

RouteSource(route_name) ==
    CASE route_name = "peer_candidate_enters_runtime_admission" -> "peer_comms"
      [] route_name = "admitted_peer_work_enters_ingress" -> "runtime_control"
      [] route_name = "peer_ingress_ready_begins_run" -> "runtime_ingress"
      [] OTHER -> "unknown_machine"

RouteEffect(route_name) ==
    CASE route_name = "peer_candidate_enters_runtime_admission" -> "EnqueueClassifiedEntry"
      [] route_name = "admitted_peer_work_enters_ingress" -> "SubmitAdmittedIngressEffect"
      [] route_name = "peer_ingress_ready_begins_run" -> "ReadyForRun"
      [] OTHER -> "unknown_effect"

RouteTargetMachine(route_name) ==
    CASE route_name = "peer_candidate_enters_runtime_admission" -> "runtime_control"
      [] route_name = "admitted_peer_work_enters_ingress" -> "runtime_ingress"
      [] route_name = "peer_ingress_ready_begins_run" -> "runtime_control"
      [] OTHER -> "unknown_machine"

RouteTargetInput(route_name) ==
    CASE route_name = "peer_candidate_enters_runtime_admission" -> "SubmitWork"
      [] route_name = "admitted_peer_work_enters_ingress" -> "AdmitQueued"
      [] route_name = "peer_ingress_ready_begins_run" -> "BeginRun"
      [] OTHER -> "unknown_input"

RouteDeliveryKind(route_name) ==
    CASE route_name = "peer_candidate_enters_runtime_admission" -> "Immediate"
      [] route_name = "admitted_peer_work_enters_ingress" -> "Immediate"
      [] route_name = "peer_ingress_ready_begins_run" -> "Immediate"
      [] OTHER -> "Unknown"

RouteTargetActor(route_name) == ActorOfMachine(RouteTargetMachine(route_name))

VARIABLES peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, model_step_count, pending_inputs, observed_inputs, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs
vars == << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, model_step_count, pending_inputs, observed_inputs, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

RoutePackets == SeqElements(pending_routes) \cup delivered_routes
PendingActors == {ActorOfMachine(packet.machine) : packet \in SeqElements(pending_inputs)}
HigherPriorityReady(actor) == \E priority \in ActorPriorities : /\ priority[2] = actor /\ priority[1] \in PendingActors

BaseInit ==
    /\ peer_comms_phase = "Ready"
    /\ runtime_control_phase = "Initializing"
    /\ runtime_control_current_run_id = None
    /\ runtime_control_pre_run_state = None
    /\ runtime_control_wake_pending = FALSE
    /\ runtime_control_process_pending = FALSE
    /\ runtime_ingress_phase = "Active"
    /\ runtime_ingress_admitted_inputs = {}
    /\ runtime_ingress_admission_order = <<>>
    /\ runtime_ingress_content_shape = [x \in {} |-> None]
    /\ runtime_ingress_request_id = [x \in {} |-> None]
    /\ runtime_ingress_reservation_key = [x \in {} |-> None]
    /\ runtime_ingress_policy_snapshot = [x \in {} |-> None]
    /\ runtime_ingress_handling_mode = [x \in {} |-> None]
    /\ runtime_ingress_lifecycle = [x \in {} |-> None]
    /\ runtime_ingress_terminal_outcome = [x \in {} |-> None]
    /\ runtime_ingress_queue = <<>>
    /\ runtime_ingress_steer_queue = <<>>
    /\ runtime_ingress_current_run = None
    /\ runtime_ingress_current_run_contributors = <<>>
    /\ runtime_ingress_last_run = [x \in {} |-> None]
    /\ runtime_ingress_last_boundary_sequence = [x \in {} |-> None]
    /\ runtime_ingress_wake_requested = FALSE
    /\ runtime_ingress_process_requested = FALSE
    /\ runtime_ingress_silent_intent_overrides = {}
    /\ model_step_count = 0
    /\ pending_routes = <<>>
    /\ delivered_routes = {}
    /\ emitted_effects = {}
    /\ observed_transitions = {}

Init ==
    /\ BaseInit
    /\ pending_inputs = <<>>
    /\ observed_inputs = {}
    /\ witness_current_script_input = None
    /\ witness_remaining_script_inputs = <<>>

WitnessInit_trusted_peer_enters_runtime ==
    /\ BaseInit
    /\ pending_inputs = <<[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:trusted_peer_enters_runtime:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]>>
    /\ observed_inputs = {[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:trusted_peer_enters_runtime:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]}
    /\ witness_current_script_input = [machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:trusted_peer_enters_runtime:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]
    /\ witness_remaining_script_inputs = <<[machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Message", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:trusted_peer_enters_runtime:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0]>>

WitnessInit_admitted_peer_work_enters_ingress ==
    /\ BaseInit
    /\ pending_inputs = <<[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:admitted_peer_work_enters_ingress:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]>>
    /\ observed_inputs = {[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:admitted_peer_work_enters_ingress:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]}
    /\ witness_current_script_input = [machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:admitted_peer_work_enters_ingress:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]
    /\ witness_remaining_script_inputs = <<[machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Request", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:admitted_peer_work_enters_ingress:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0], [machine |-> "runtime_control", variant |-> "AdmissionAccepted", payload |-> [admission_effect |-> "SubmitAdmittedIngressEffect", content_shape |-> "InlineImage", handling_mode |-> "Steer", request_id |-> None, reservation_key |-> None, work_id |-> "raw_1"], source_kind |-> "entry", source_route |-> "witness:admitted_peer_work_enters_ingress:3", source_machine |-> "external_entry", source_effect |-> "AdmissionAccepted", effect_id |-> 0]>>

WitnessInit_peer_ingress_ready_begins_run ==
    /\ BaseInit
    /\ pending_inputs = <<[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]>>
    /\ observed_inputs = {[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]}
    /\ witness_current_script_input = [machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]
    /\ witness_remaining_script_inputs = <<[machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Request", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0], [machine |-> "runtime_control", variant |-> "AdmissionAccepted", payload |-> [admission_effect |-> "SubmitAdmittedIngressEffect", content_shape |-> "TextOnly", handling_mode |-> "Steer", request_id |-> None, reservation_key |-> None, work_id |-> "raw_1"], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:3", source_machine |-> "external_entry", source_effect |-> "AdmissionAccepted", effect_id |-> 0], [machine |-> "runtime_ingress", variant |-> "StageDrainSnapshot", payload |-> [contributing_work_ids |-> <<"raw_1">>, run_id |-> "runid_1"], source_kind |-> "entry", source_route |-> "witness:peer_ingress_ready_begins_run:4", source_machine |-> "external_entry", source_effect |-> "StageDrainSnapshot", effect_id |-> 0]>>

WitnessInit_control_preempts_peer_delivery ==
    /\ BaseInit
    /\ pending_inputs = <<[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:control_preempts_peer_delivery:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0], [machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Message", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:control_preempts_peer_delivery:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0]>>
    /\ observed_inputs = {[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "witness:control_preempts_peer_delivery:1", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0], [machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Message", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:control_preempts_peer_delivery:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0]}
    /\ witness_current_script_input = [machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> FALSE, fallback_sender_name |-> "peer_1", handling_mode |-> "Steer", handling_mode_present |-> TRUE, intent |-> "review", kind |-> "Message", lifecycle_peer |-> "", lifecycle_peer_present |-> FALSE, raw_item_id |-> "raw_1", require_peer_auth |-> FALSE, sender_name |-> "peer_1", sender_name_known |-> TRUE, silent_intent |-> FALSE], source_kind |-> "entry", source_route |-> "witness:control_preempts_peer_delivery:2", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0]
    /\ witness_remaining_script_inputs = <<>>

peer_comms__NormalizedHandlingMode(handling_mode_present, handling_mode) == (IF handling_mode_present THEN handling_mode ELSE "Queue")

peer_comms__EffectiveSender(sender_name_known, sender_name, fallback_sender_name) == (IF sender_name_known THEN Some(sender_name) ELSE Some(fallback_sender_name))

peer_comms__EffectiveLifecyclePeer(lifecycle_peer_present, lifecycle_peer, sender_name_known, sender_name, fallback_sender_name) == (IF lifecycle_peer_present THEN Some(lifecycle_peer) ELSE peer_comms__EffectiveSender(sender_name_known, sender_name, fallback_sender_name))

peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ (packet.payload.require_peer_auth = TRUE)
       /\ (packet.payload.sender_name_known = FALSE)
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "DropIngress", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "DropUntrustedExternal"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "DropUntrustedExternal", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ (packet.payload.kind = "Ack")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "DropIngress", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "DropAckExternal"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "DropAckExternal", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Message")
       /\ (packet.payload.dismiss_message = TRUE)
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "SetDismissFlag", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "DismissExternalMessage"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "DismissExternalMessage", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.intent = "mob.peer_added")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleAdded"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PeerLifecycleAdded", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> peer_comms__EffectiveLifecyclePeer(packet.payload.lifecycle_peer_present, packet.payload.lifecycle_peer, packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleAdded"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueLifecycleAdded", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.intent = "mob.peer_retired")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleRetired"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PeerLifecycleRetired", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> peer_comms__EffectiveLifecyclePeer(packet.payload.lifecycle_peer_present, packet.payload.lifecycle_peer, packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueLifecycleRetired", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.intent = "mob.peer_unwired")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleUnwired"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PeerLifecycleUnwired", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> peer_comms__EffectiveLifecyclePeer(packet.payload.lifecycle_peer_present, packet.payload.lifecycle_peer, packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleUnwired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueLifecycleUnwired", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.intent = "mob.kickoff_failed")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleKickoffFailed"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PeerLifecycleKickoffFailed", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> peer_comms__EffectiveLifecyclePeer(packet.payload.lifecycle_peer_present, packet.payload.lifecycle_peer, packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleKickoffFailed"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueLifecycleKickoffFailed", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.intent = "mob.kickoff_cancelled")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleKickoffCancelled"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PeerLifecycleKickoffCancelled", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> peer_comms__EffectiveLifecyclePeer(packet.payload.lifecycle_peer_present, packet.payload.lifecycle_peer, packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueLifecycleKickoffCancelled"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueLifecycleKickoffCancelled", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.silent_intent = TRUE)
       /\ ((packet.payload.intent # "mob.peer_added") /\ (packet.payload.intent # "mob.peer_retired") /\ (packet.payload.intent # "mob.peer_unwired") /\ (packet.payload.intent # "mob.kickoff_failed") /\ (packet.payload.intent # "mob.kickoff_cancelled"))
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueSilentRequest"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "SilentRequest", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> None, normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueSilentRequest"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueSilentRequest", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Request")
       /\ (packet.payload.silent_intent = FALSE)
       /\ ((packet.payload.intent # "mob.peer_added") /\ (packet.payload.intent # "mob.peer_retired") /\ (packet.payload.intent # "mob.peer_unwired") /\ (packet.payload.intent # "mob.kickoff_failed") /\ (packet.payload.intent # "mob.kickoff_cancelled"))
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueActionableRequest"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "ActionableRequest", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> None, normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueActionableRequest"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueActionableRequest", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Message")
       /\ (packet.payload.dismiss_message = FALSE)
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueActionableMessage"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "ActionableMessage", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> None, normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueActionableMessage"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueActionableMessage", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyExternalEnvelope"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.require_peer_auth = arg_require_peer_auth
       /\ packet.payload.sender_name_known = arg_sender_name_known
       /\ packet.payload.sender_name = arg_sender_name
       /\ packet.payload.fallback_sender_name = arg_fallback_sender_name
       /\ packet.payload.kind = arg_kind
       /\ packet.payload.intent = arg_intent
       /\ packet.payload.lifecycle_peer_present = arg_lifecycle_peer_present
       /\ packet.payload.lifecycle_peer = arg_lifecycle_peer
       /\ packet.payload.handling_mode_present = arg_handling_mode_present
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.silent_intent = arg_silent_intent
       /\ packet.payload.dismiss_message = arg_dismiss_message
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ ~(((packet.payload.require_peer_auth = TRUE) /\ (packet.payload.sender_name_known = FALSE)))
       /\ (packet.payload.kind = "Response")
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueResponse"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "Response", from_peer |-> peer_comms__EffectiveSender(packet.payload.sender_name_known, packet.payload.sender_name, packet.payload.fallback_sender_name), lifecycle_peer |-> None, normalized_handling_mode |-> peer_comms__NormalizedHandlingMode(packet.payload.handling_mode_present, packet.payload.handling_mode), raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueueResponse"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueueResponse", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "peer_comms"
       /\ packet.variant = "ClassifyPlainEvent"
       /\ packet.payload.raw_item_id = arg_raw_item_id
       /\ packet.payload.source_name = arg_source_name
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ ~HigherPriorityReady("peer_plane")
       /\ peer_comms_phase = "Ready"
       /\ peer_comms_phase' = "Ready"
       /\ UNCHANGED << runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> packet.payload.handling_mode, request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> packet.payload.handling_mode, request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], source_kind |-> "route", source_route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", source_effect |-> "EnqueueClassifiedEntry", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_candidate_enters_runtime_admission", source_machine |-> "peer_comms", effect |-> "EnqueueClassifiedEntry", target_machine |-> "runtime_control", target_input |-> "SubmitWork", payload |-> [content_shape |-> "TextOnly", handling_mode |-> packet.payload.handling_mode, request_id |-> None, reservation_key |-> None, work_id |-> packet.payload.raw_item_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "EnqueuePlainEvent"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "peer_comms", variant |-> "EnqueueClassifiedEntry", payload |-> [class |-> "PlainEvent", from_peer |-> None, lifecycle_peer |-> None, normalized_handling_mode |-> packet.payload.handling_mode, raw_item_id |-> packet.payload.raw_item_id], effect_id |-> (model_step_count + 1), source_transition |-> "EnqueuePlainEvent"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "peer_comms", transition |-> "EnqueuePlainEvent", actor |-> "peer_plane", step |-> (model_step_count + 1), from_phase |-> peer_comms_phase, to_phase |-> "Ready"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_Initialize ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "Initialize"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Initializing"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "Initialize", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AttachFromIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AttachExecutor"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Attached"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AttachFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_DetachToIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "DetachExecutor"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "DetachToIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_BeginRunFromIdle(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "BeginRun"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_current_run_id' = Some(packet.payload.run_id)
       /\ runtime_control_pre_run_state' = Some("Idle")
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitRunPrimitive", payload |-> [run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "BeginRunFromIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "BeginRunFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_BeginRunFromRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "BeginRun"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_current_run_id' = Some(packet.payload.run_id)
       /\ runtime_control_pre_run_state' = Some("Retired")
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitRunPrimitive", payload |-> [run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "BeginRunFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "BeginRunFromRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_BeginRunFromAttached(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "BeginRun"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_current_run_id' = Some(packet.payload.run_id)
       /\ runtime_control_pre_run_state' = Some("Attached")
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitRunPrimitive", payload |-> [run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "BeginRunFromAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "BeginRunFromAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_BeginRunFromRecovering(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "BeginRun"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Recovering"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_current_run_id' = Some(packet.payload.run_id)
       /\ runtime_control_pre_run_state' = Some("Recovering")
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitRunPrimitive", payload |-> [run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "BeginRunFromRecovering"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "BeginRunFromRecovering", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCompletedToIdle(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ ((runtime_control_pre_run_state = None) \/ (runtime_control_pre_run_state = Some("Idle")) \/ (runtime_control_pre_run_state = Some("Recovering")))
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCompletedToIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCompletedToAttached(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Attached"))
       /\ runtime_control_phase' = "Attached"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCompletedToAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCompletedToRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Retired"))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCompletedToRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunFailedToIdle(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ ((runtime_control_pre_run_state = None) \/ (runtime_control_pre_run_state = Some("Idle")) \/ (runtime_control_pre_run_state = Some("Recovering")))
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunFailedToIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunFailedToAttached(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Attached"))
       /\ runtime_control_phase' = "Attached"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunFailedToAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunFailedToRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Retired"))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunFailedToRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCancelledToIdle(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ ((runtime_control_pre_run_state = None) \/ (runtime_control_pre_run_state = Some("Idle")) \/ (runtime_control_pre_run_state = Some("Recovering")))
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCancelledToIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCancelledToAttached(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Attached"))
       /\ runtime_control_phase' = "Attached"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCancelledToAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCancelledToRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ (runtime_control_pre_run_state = Some("Retired"))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCancelledToRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCompletedFromRetiredInFlight(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCompletedFromRetiredInFlight", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunFailedFromRetiredInFlight(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunFailedFromRetiredInFlight", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RunCancelledFromRetiredInFlight(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ (runtime_control_current_run_id = Some(packet.payload.run_id))
       /\ runtime_control_phase' = "Retired"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RunCancelledFromRetiredInFlight", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecoverRequestedFromIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecoverRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_pre_run_state' = Some("Idle")
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecoverRequestedFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecoverRequestedFromRunning ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecoverRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = Some("Running")
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecoverRequestedFromRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecoverRequestedFromAttached ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecoverRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_pre_run_state' = Some("Attached")
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecoverRequestedFromAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecoverySucceeded ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecoverySucceeded"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Recovering"
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecoverySucceeded", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RetireRequestedFromIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RetireRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Retired"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Retire"], effect_id |-> (model_step_count + 1), source_transition |-> "RetireRequestedFromIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RetireRequestedFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RetireRequestedFromRunning ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RetireRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Retired"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Retire"], effect_id |-> (model_step_count + 1), source_transition |-> "RetireRequestedFromRunning"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RetireRequestedFromRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RetireRequestedFromAttached ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RetireRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Retired"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Retire"], effect_id |-> (model_step_count + 1), source_transition |-> "RetireRequestedFromAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RetireRequestedFromAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ResetRequested ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ResetRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Initializing" \/ runtime_control_phase = "Idle" \/ runtime_control_phase = "Attached" \/ runtime_control_phase = "Recovering" \/ runtime_control_phase = "Retired"
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Reset"], effect_id |-> (model_step_count + 1), source_transition |-> "ResetRequested"], [machine |-> "runtime_control", variant |-> "ResolveCompletionAsTerminated", payload |-> [reason |-> "Reset"], effect_id |-> (model_step_count + 1), source_transition |-> "ResetRequested"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ResetRequested", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_StopRequested ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "StopRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Initializing" \/ runtime_control_phase = "Idle" \/ runtime_control_phase = "Attached" \/ runtime_control_phase = "Running" \/ runtime_control_phase = "Recovering" \/ runtime_control_phase = "Retired"
       /\ runtime_control_phase' = "Stopped"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Stop"], effect_id |-> (model_step_count + 1), source_transition |-> "StopRequested"], [machine |-> "runtime_control", variant |-> "ResolveCompletionAsTerminated", payload |-> [reason |-> "Stopped"], effect_id |-> (model_step_count + 1), source_transition |-> "StopRequested"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "StopRequested", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Stopped"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_DestroyRequested ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "DestroyRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Initializing" \/ runtime_control_phase = "Idle" \/ runtime_control_phase = "Attached" \/ runtime_control_phase = "Running" \/ runtime_control_phase = "Recovering" \/ runtime_control_phase = "Retired" \/ runtime_control_phase = "Stopped"
       /\ runtime_control_phase' = "Destroyed"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ UNCHANGED << peer_comms_phase, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ApplyControlPlaneCommand", payload |-> [command |-> "Destroy"], effect_id |-> (model_step_count + 1), source_transition |-> "DestroyRequested"], [machine |-> "runtime_control", variant |-> "ResolveCompletionAsTerminated", payload |-> [reason |-> "Destroyed"], effect_id |-> (model_step_count + 1), source_transition |-> "DestroyRequested"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "DestroyRequested", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Destroyed"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ResumeRequested ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ResumeRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Recovering"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ResumeRequested", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "SubmitWork"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ResolveAdmission", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SubmitWorkFromIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "SubmitWorkFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "SubmitWork"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Running"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ResolveAdmission", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SubmitWorkFromRunning"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "SubmitWorkFromRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "SubmitWork"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Attached"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "ResolveAdmission", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SubmitWorkFromAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "SubmitWorkFromAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ (packet.payload.handling_mode = "Queue")
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_wake_pending' = TRUE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleQueue"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleQueue"], [machine |-> "runtime_control", variant |-> "SignalWake", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleQueue"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedIdleQueue", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ (packet.payload.handling_mode = "Steer")
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_wake_pending' = TRUE
       /\ runtime_control_process_pending' = TRUE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleSteer"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleSteer"], [machine |-> "runtime_control", variant |-> "SignalWake", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleSteer"], [machine |-> "runtime_control", variant |-> "SignalImmediateProcess", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedIdleSteer"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedIdleSteer", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (packet.payload.handling_mode = "Queue")
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningQueue"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningQueue"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedRunningQueue", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ (packet.payload.handling_mode = "Steer")
       /\ runtime_control_phase' = "Running"
       /\ runtime_control_wake_pending' = TRUE
       /\ runtime_control_process_pending' = TRUE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningSteer"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningSteer"], [machine |-> "runtime_control", variant |-> "SignalWake", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningSteer"], [machine |-> "runtime_control", variant |-> "SignalImmediateProcess", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedRunningSteer"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedRunningSteer", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ (packet.payload.handling_mode = "Queue")
       /\ runtime_control_phase' = "Attached"
       /\ runtime_control_wake_pending' = TRUE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedQueue"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedQueue"], [machine |-> "runtime_control", variant |-> "SignalWake", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedQueue"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedAttachedQueue", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionAccepted"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.admission_effect = arg_admission_effect
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ (packet.payload.handling_mode = "Steer")
       /\ runtime_control_phase' = "Attached"
       /\ runtime_control_wake_pending' = TRUE
       /\ runtime_control_process_pending' = TRUE
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], source_kind |-> "route", source_route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", source_effect |-> "SubmitAdmittedIngressEffect", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "admitted_peer_work_enters_ingress", source_machine |-> "runtime_control", effect |-> "SubmitAdmittedIngressEffect", target_machine |-> "runtime_ingress", target_input |-> "AdmitQueued", payload |-> [content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, policy |-> "PeerQueued", request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], actor |-> "ordinary_ingress", effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedSteer"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "SubmitAdmittedIngressEffect", payload |-> [admission_effect |-> packet.payload.admission_effect, content_shape |-> packet.payload.content_shape, handling_mode |-> packet.payload.handling_mode, request_id |-> packet.payload.request_id, reservation_key |-> packet.payload.reservation_key, work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedSteer"], [machine |-> "runtime_control", variant |-> "SignalWake", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedSteer"], [machine |-> "runtime_control", variant |-> "SignalImmediateProcess", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionAcceptedAttachedSteer"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionAcceptedAttachedSteer", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionRejected"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.reason = arg_reason
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> packet.payload.reason, kind |-> "AdmissionRejected"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionRejectedIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionRejectedIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionRejected"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.reason = arg_reason
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Running"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> packet.payload.reason, kind |-> "AdmissionRejected"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionRejectedRunning"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionRejectedRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionRejected"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.reason = arg_reason
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Attached"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> packet.payload.reason, kind |-> "AdmissionRejected"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionRejectedAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionRejectedAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionDeduplicated"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.existing_work_id = arg_existing_work_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "ExistingInputLinked", kind |-> "AdmissionDeduplicated"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionDeduplicatedIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionDeduplicatedIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionDeduplicated"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.existing_work_id = arg_existing_work_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Running"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "ExistingInputLinked", kind |-> "AdmissionDeduplicated"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionDeduplicatedRunning"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionDeduplicatedRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "AdmissionDeduplicated"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.existing_work_id = arg_existing_work_id
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Attached"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "ExistingInputLinked", kind |-> "AdmissionDeduplicated"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmissionDeduplicatedAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "AdmissionDeduplicatedAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ExternalToolDeltaReceivedIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ExternalToolDeltaReceived"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ runtime_control_phase' = "Idle"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Received", kind |-> "ExternalToolDelta"], effect_id |-> (model_step_count + 1), source_transition |-> "ExternalToolDeltaReceivedIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ExternalToolDeltaReceivedIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ExternalToolDeltaReceivedRunning ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ExternalToolDeltaReceived"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Running"
       /\ runtime_control_phase' = "Running"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Received", kind |-> "ExternalToolDelta"], effect_id |-> (model_step_count + 1), source_transition |-> "ExternalToolDeltaReceivedRunning"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ExternalToolDeltaReceivedRunning", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Running"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ExternalToolDeltaReceivedRecovering ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ExternalToolDeltaReceived"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Recovering"
       /\ runtime_control_phase' = "Recovering"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Received", kind |-> "ExternalToolDelta"], effect_id |-> (model_step_count + 1), source_transition |-> "ExternalToolDeltaReceivedRecovering"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ExternalToolDeltaReceivedRecovering", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ExternalToolDeltaReceivedRetired ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ExternalToolDeltaReceived"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ runtime_control_phase' = "Retired"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Received", kind |-> "ExternalToolDelta"], effect_id |-> (model_step_count + 1), source_transition |-> "ExternalToolDeltaReceivedRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ExternalToolDeltaReceivedRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_ExternalToolDeltaReceivedAttached ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "ExternalToolDeltaReceived"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ runtime_control_phase' = "Attached"
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Received", kind |-> "ExternalToolDelta"], effect_id |-> (model_step_count + 1), source_transition |-> "ExternalToolDeltaReceivedAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "ExternalToolDeltaReceivedAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Attached"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecycleRequestedFromRetired ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecycleRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Retired"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_pre_run_state' = Some("Retired")
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "InitiateRecycle", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "RecycleRequestedFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecycleRequestedFromRetired", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecycleRequestedFromIdle ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecycleRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Idle"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_pre_run_state' = Some("Idle")
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "InitiateRecycle", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "RecycleRequestedFromIdle"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecycleRequestedFromIdle", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecycleRequestedFromAttached ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecycleRequested"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Attached"
       /\ (runtime_control_current_run_id = None)
       /\ runtime_control_phase' = "Recovering"
       /\ runtime_control_pre_run_state' = Some("Attached")
       /\ UNCHANGED << peer_comms_phase, runtime_control_current_run_id, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "InitiateRecycle", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "RecycleRequestedFromAttached"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecycleRequestedFromAttached", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Recovering"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_RecycleSucceeded ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_control"
       /\ packet.variant = "RecycleSucceeded"
       /\ ~HigherPriorityReady("control_plane")
       /\ runtime_control_phase = "Recovering"
       /\ runtime_control_phase' = "Idle"
       /\ runtime_control_current_run_id' = None
       /\ runtime_control_pre_run_state' = None
       /\ runtime_control_wake_pending' = FALSE
       /\ runtime_control_process_pending' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_control", variant |-> "EmitRuntimeNotice", payload |-> [detail |-> "Succeeded", kind |-> "Recycle"], effect_id |-> (model_step_count + 1), source_transition |-> "RecycleSucceeded"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_control", transition |-> "RecycleSucceeded", actor |-> "control_plane", step |-> (model_step_count + 1), from_phase |-> runtime_control_phase, to_phase |-> "Idle"]}
       /\ model_step_count' = model_step_count + 1


runtime_control_running_implies_active_run == ((runtime_control_phase # "Running") \/ (runtime_control_current_run_id # None))
runtime_control_active_run_only_while_running_or_retired == ((runtime_control_current_run_id = None) \/ (runtime_control_phase = "Running") \/ (runtime_control_phase = "Retired"))

RECURSIVE runtime_ingress_StageDrainSnapshotFromActive_ForEach0_last_run(_, _, _)
runtime_ingress_StageDrainSnapshotFromActive_ForEach0_last_run(acc, items, outer_run_id) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some(outer_run_id)) IN runtime_ingress_StageDrainSnapshotFromActive_ForEach0_last_run(next_acc, Tail(items), outer_run_id)

RECURSIVE runtime_ingress_StageDrainSnapshotFromActive_ForEach0_lifecycle(_, _, _)
runtime_ingress_StageDrainSnapshotFromActive_ForEach0_lifecycle(acc, items, outer_run_id) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Staged") IN runtime_ingress_StageDrainSnapshotFromActive_ForEach0_lifecycle(next_acc, Tail(items), outer_run_id)

RECURSIVE runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_last_run(_, _, _)
runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_last_run(acc, items, outer_run_id) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some(outer_run_id)) IN runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_last_run(next_acc, Tail(items), outer_run_id)

RECURSIVE runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_lifecycle(_, _, _)
runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_lifecycle(acc, items, outer_run_id) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Staged") IN runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_lifecycle(next_acc, Tail(items), outer_run_id)

RECURSIVE runtime_ingress_BoundaryAppliedFromActive_ForEach2_last_boundary_sequence(_, _, _)
runtime_ingress_BoundaryAppliedFromActive_ForEach2_last_boundary_sequence(acc, items, outer_boundary_sequence) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some(outer_boundary_sequence)) IN runtime_ingress_BoundaryAppliedFromActive_ForEach2_last_boundary_sequence(next_acc, Tail(items), outer_boundary_sequence)

RECURSIVE runtime_ingress_BoundaryAppliedFromActive_ForEach2_lifecycle(_, _, _)
runtime_ingress_BoundaryAppliedFromActive_ForEach2_lifecycle(acc, items, outer_boundary_sequence) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "AppliedPendingConsumption") IN runtime_ingress_BoundaryAppliedFromActive_ForEach2_lifecycle(next_acc, Tail(items), outer_boundary_sequence)

RECURSIVE runtime_ingress_BoundaryAppliedFromRetired_ForEach3_last_boundary_sequence(_, _, _)
runtime_ingress_BoundaryAppliedFromRetired_ForEach3_last_boundary_sequence(acc, items, outer_boundary_sequence) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some(outer_boundary_sequence)) IN runtime_ingress_BoundaryAppliedFromRetired_ForEach3_last_boundary_sequence(next_acc, Tail(items), outer_boundary_sequence)

RECURSIVE runtime_ingress_BoundaryAppliedFromRetired_ForEach3_lifecycle(_, _, _)
runtime_ingress_BoundaryAppliedFromRetired_ForEach3_lifecycle(acc, items, outer_boundary_sequence) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "AppliedPendingConsumption") IN runtime_ingress_BoundaryAppliedFromRetired_ForEach3_lifecycle(next_acc, Tail(items), outer_boundary_sequence)

RECURSIVE runtime_ingress_RunCompletedFromActive_ForEach4_lifecycle(_, _)
runtime_ingress_RunCompletedFromActive_ForEach4_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Consumed") IN runtime_ingress_RunCompletedFromActive_ForEach4_lifecycle(next_acc, Tail(items))

RECURSIVE runtime_ingress_RunCompletedFromActive_ForEach4_terminal_outcome(_, _)
runtime_ingress_RunCompletedFromActive_ForEach4_terminal_outcome(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some("Consumed")) IN runtime_ingress_RunCompletedFromActive_ForEach4_terminal_outcome(next_acc, Tail(items))

RECURSIVE runtime_ingress_RunCompletedFromRetired_ForEach5_lifecycle(_, _)
runtime_ingress_RunCompletedFromRetired_ForEach5_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Consumed") IN runtime_ingress_RunCompletedFromRetired_ForEach5_lifecycle(next_acc, Tail(items))

RECURSIVE runtime_ingress_RunCompletedFromRetired_ForEach5_terminal_outcome(_, _)
runtime_ingress_RunCompletedFromRetired_ForEach5_terminal_outcome(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some("Consumed")) IN runtime_ingress_RunCompletedFromRetired_ForEach5_terminal_outcome(next_acc, Tail(items))

RECURSIVE runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(_, _, _)
runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(acc, items, captured_handling_mode) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") = "Staged") THEN MapSet(acc, work_id, "Queued") ELSE acc IN runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(next_acc, Tail(items), captured_handling_mode)

RECURSIVE runtime_ingress_RunFailedFromActive_ForEach6_queue(_, _, _, _)
runtime_ingress_RunFailedFromActive_ForEach6_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN acc ELSE (<<work_id>> \o acc) ELSE acc IN runtime_ingress_RunFailedFromActive_ForEach6_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunFailedFromActive_ForEach6_steer_queue(_, _, _, _)
runtime_ingress_RunFailedFromActive_ForEach6_steer_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN (<<work_id>> \o acc) ELSE acc ELSE acc IN runtime_ingress_RunFailedFromActive_ForEach6_steer_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(_, _, _)
runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(acc, items, captured_handling_mode) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") = "Staged") THEN MapSet(acc, work_id, "Queued") ELSE acc IN runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(next_acc, Tail(items), captured_handling_mode)

RECURSIVE runtime_ingress_RunFailedFromRetired_ForEach7_queue(_, _, _, _)
runtime_ingress_RunFailedFromRetired_ForEach7_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN acc ELSE (<<work_id>> \o acc) ELSE acc IN runtime_ingress_RunFailedFromRetired_ForEach7_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunFailedFromRetired_ForEach7_steer_queue(_, _, _, _)
runtime_ingress_RunFailedFromRetired_ForEach7_steer_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN (<<work_id>> \o acc) ELSE acc ELSE acc IN runtime_ingress_RunFailedFromRetired_ForEach7_steer_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(_, _, _)
runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(acc, items, captured_handling_mode) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") = "Staged") THEN MapSet(acc, work_id, "Queued") ELSE acc IN runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(next_acc, Tail(items), captured_handling_mode)

RECURSIVE runtime_ingress_RunCancelledFromActive_ForEach8_queue(_, _, _, _)
runtime_ingress_RunCancelledFromActive_ForEach8_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN acc ELSE (<<work_id>> \o acc) ELSE acc IN runtime_ingress_RunCancelledFromActive_ForEach8_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunCancelledFromActive_ForEach8_steer_queue(_, _, _, _)
runtime_ingress_RunCancelledFromActive_ForEach8_steer_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN (<<work_id>> \o acc) ELSE acc ELSE acc IN runtime_ingress_RunCancelledFromActive_ForEach8_steer_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(_, _, _)
runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(acc, items, captured_handling_mode) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") = "Staged") THEN MapSet(acc, work_id, "Queued") ELSE acc IN runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(next_acc, Tail(items), captured_handling_mode)

RECURSIVE runtime_ingress_RunCancelledFromRetired_ForEach9_queue(_, _, _, _)
runtime_ingress_RunCancelledFromRetired_ForEach9_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN acc ELSE (<<work_id>> \o acc) ELSE acc IN runtime_ingress_RunCancelledFromRetired_ForEach9_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_RunCancelledFromRetired_ForEach9_steer_queue(_, _, _, _)
runtime_ingress_RunCancelledFromRetired_ForEach9_steer_queue(acc, items, captured_handling_mode, captured_lifecycle) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == IF ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") = "Staged") THEN IF ((IF work_id \in DOMAIN captured_handling_mode THEN captured_handling_mode[work_id] ELSE "None") = "Steer") THEN (<<work_id>> \o acc) ELSE acc ELSE acc IN runtime_ingress_RunCancelledFromRetired_ForEach9_steer_queue(next_acc, Tail(items), captured_handling_mode, captured_lifecycle)

RECURSIVE runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_lifecycle(_, _)
runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Coalesced") IN runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_lifecycle(next_acc, Tail(items))

RECURSIVE runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_terminal_outcome(_, _)
runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_terminal_outcome(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some("Coalesced")) IN runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_terminal_outcome(next_acc, Tail(items))

RECURSIVE runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_lifecycle(_, _)
runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Coalesced") IN runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_lifecycle(next_acc, Tail(items))

RECURSIVE runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_terminal_outcome(_, _)
runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_terminal_outcome(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, Some("Coalesced")) IN runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_terminal_outcome(next_acc, Tail(items))

RECURSIVE runtime_ingress_ResetFromActive_ForEach12_lifecycle(_, _, _)
runtime_ingress_ResetFromActive_ForEach12_lifecycle(acc, remaining, captured_terminal_outcome) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN captured_terminal_outcome THEN captured_terminal_outcome[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, "Abandoned") ELSE acc IN runtime_ingress_ResetFromActive_ForEach12_lifecycle(next_acc, remaining \ {item}, captured_terminal_outcome)

RECURSIVE runtime_ingress_ResetFromActive_ForEach12_terminal_outcome(_, _, _)
runtime_ingress_ResetFromActive_ForEach12_terminal_outcome(acc, remaining, captured_lifecycle) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, Some("AbandonedReset")) ELSE acc IN runtime_ingress_ResetFromActive_ForEach12_terminal_outcome(next_acc, remaining \ {item}, captured_lifecycle)

RECURSIVE runtime_ingress_ResetFromRetired_ForEach13_lifecycle(_, _, _)
runtime_ingress_ResetFromRetired_ForEach13_lifecycle(acc, remaining, captured_terminal_outcome) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN captured_terminal_outcome THEN captured_terminal_outcome[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, "Abandoned") ELSE acc IN runtime_ingress_ResetFromRetired_ForEach13_lifecycle(next_acc, remaining \ {item}, captured_terminal_outcome)

RECURSIVE runtime_ingress_ResetFromRetired_ForEach13_terminal_outcome(_, _, _)
runtime_ingress_ResetFromRetired_ForEach13_terminal_outcome(acc, remaining, captured_lifecycle) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, Some("AbandonedReset")) ELSE acc IN runtime_ingress_ResetFromRetired_ForEach13_terminal_outcome(next_acc, remaining \ {item}, captured_lifecycle)

RECURSIVE runtime_ingress_Destroy_ForEach14_lifecycle(_, _, _)
runtime_ingress_Destroy_ForEach14_lifecycle(acc, remaining, captured_terminal_outcome) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN captured_terminal_outcome THEN captured_terminal_outcome[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, "Abandoned") ELSE acc IN runtime_ingress_Destroy_ForEach14_lifecycle(next_acc, remaining \ {item}, captured_terminal_outcome)

RECURSIVE runtime_ingress_Destroy_ForEach14_terminal_outcome(_, _, _)
runtime_ingress_Destroy_ForEach14_terminal_outcome(acc, remaining, captured_lifecycle) == IF remaining = {} THEN acc ELSE LET item == CHOOSE x \in remaining : TRUE IN LET work_id == item IN LET next_acc == IF (((IF work_id \in DOMAIN acc THEN acc[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN captured_lifecycle THEN captured_lifecycle[work_id] ELSE "None") # "Abandoned")) THEN MapSet(acc, work_id, Some("AbandonedDestroyed")) ELSE acc IN runtime_ingress_Destroy_ForEach14_terminal_outcome(next_acc, remaining \ {item}, captured_lifecycle)

RECURSIVE runtime_ingress_RecoverFromActive_ForEach15_lifecycle(_, _)
runtime_ingress_RecoverFromActive_ForEach15_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Queued") IN runtime_ingress_RecoverFromActive_ForEach15_lifecycle(next_acc, Tail(items))

RECURSIVE runtime_ingress_RecoverFromRetired_ForEach16_lifecycle(_, _)
runtime_ingress_RecoverFromRetired_ForEach16_lifecycle(acc, items) == IF Len(items) = 0 THEN acc ELSE LET work_id == Head(items) IN LET next_acc == MapSet(acc, work_id, "Queued") IN runtime_ingress_RecoverFromRetired_ForEach16_lifecycle(next_acc, Tail(items))

runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "AdmitQueued"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.policy = arg_policy
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ ~((packet.payload.work_id \in runtime_ingress_admitted_inputs))
       /\ (packet.payload.handling_mode = "Queue")
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_admitted_inputs' = (runtime_ingress_admitted_inputs \cup {packet.payload.work_id})
       /\ runtime_ingress_admission_order' = Append(runtime_ingress_admission_order, packet.payload.work_id)
       /\ runtime_ingress_content_shape' = MapSet(runtime_ingress_content_shape, packet.payload.work_id, packet.payload.content_shape)
       /\ runtime_ingress_request_id' = MapSet(runtime_ingress_request_id, packet.payload.work_id, packet.payload.request_id)
       /\ runtime_ingress_reservation_key' = MapSet(runtime_ingress_reservation_key, packet.payload.work_id, packet.payload.reservation_key)
       /\ runtime_ingress_policy_snapshot' = MapSet(runtime_ingress_policy_snapshot, packet.payload.work_id, packet.payload.policy)
       /\ runtime_ingress_handling_mode' = MapSet(runtime_ingress_handling_mode, packet.payload.work_id, packet.payload.handling_mode)
       /\ runtime_ingress_lifecycle' = MapSet(runtime_ingress_lifecycle, packet.payload.work_id, "Queued")
       /\ runtime_ingress_terminal_outcome' = MapSet(runtime_ingress_terminal_outcome, packet.payload.work_id, None)
       /\ runtime_ingress_queue' = Append(runtime_ingress_queue, packet.payload.work_id)
       /\ runtime_ingress_last_run' = MapSet(runtime_ingress_last_run, packet.payload.work_id, None)
       /\ runtime_ingress_last_boundary_sequence' = MapSet(runtime_ingress_last_boundary_sequence, packet.payload.work_id, None)
       /\ runtime_ingress_wake_requested' = TRUE
       /\ runtime_ingress_process_requested' = (runtime_ingress_process_requested \/ FALSE)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressAccepted", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedQueue"], [machine |-> "runtime_ingress", variant |-> "InputLifecycleNotice", payload |-> [new_state |-> "Queued", work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedQueue"], [machine |-> "runtime_ingress", variant |-> "WakeRuntime", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedQueue"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "AdmitQueuedQueue", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "AdmitQueued"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.handling_mode = arg_handling_mode
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.policy = arg_policy
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ ~((packet.payload.work_id \in runtime_ingress_admitted_inputs))
       /\ (packet.payload.handling_mode = "Steer")
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_admitted_inputs' = (runtime_ingress_admitted_inputs \cup {packet.payload.work_id})
       /\ runtime_ingress_admission_order' = Append(runtime_ingress_admission_order, packet.payload.work_id)
       /\ runtime_ingress_content_shape' = MapSet(runtime_ingress_content_shape, packet.payload.work_id, packet.payload.content_shape)
       /\ runtime_ingress_request_id' = MapSet(runtime_ingress_request_id, packet.payload.work_id, packet.payload.request_id)
       /\ runtime_ingress_reservation_key' = MapSet(runtime_ingress_reservation_key, packet.payload.work_id, packet.payload.reservation_key)
       /\ runtime_ingress_policy_snapshot' = MapSet(runtime_ingress_policy_snapshot, packet.payload.work_id, packet.payload.policy)
       /\ runtime_ingress_handling_mode' = MapSet(runtime_ingress_handling_mode, packet.payload.work_id, packet.payload.handling_mode)
       /\ runtime_ingress_lifecycle' = MapSet(runtime_ingress_lifecycle, packet.payload.work_id, "Queued")
       /\ runtime_ingress_terminal_outcome' = MapSet(runtime_ingress_terminal_outcome, packet.payload.work_id, None)
       /\ runtime_ingress_steer_queue' = Append(runtime_ingress_steer_queue, packet.payload.work_id)
       /\ runtime_ingress_last_run' = MapSet(runtime_ingress_last_run, packet.payload.work_id, None)
       /\ runtime_ingress_last_boundary_sequence' = MapSet(runtime_ingress_last_boundary_sequence, packet.payload.work_id, None)
       /\ runtime_ingress_wake_requested' = TRUE
       /\ runtime_ingress_process_requested' = (runtime_ingress_process_requested \/ TRUE)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressAccepted", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedSteer"], [machine |-> "runtime_ingress", variant |-> "InputLifecycleNotice", payload |-> [new_state |-> "Queued", work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedSteer"], [machine |-> "runtime_ingress", variant |-> "WakeRuntime", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedSteer"], [machine |-> "runtime_ingress", variant |-> "RequestImmediateProcessing", payload |-> [tag |-> "unit"], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitQueuedSteer"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "AdmitQueuedSteer", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "AdmitConsumedOnAccept"
       /\ packet.payload.work_id = arg_work_id
       /\ packet.payload.content_shape = arg_content_shape
       /\ packet.payload.request_id = arg_request_id
       /\ packet.payload.reservation_key = arg_reservation_key
       /\ packet.payload.policy = arg_policy
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ ~((packet.payload.work_id \in runtime_ingress_admitted_inputs))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_admitted_inputs' = (runtime_ingress_admitted_inputs \cup {packet.payload.work_id})
       /\ runtime_ingress_admission_order' = Append(runtime_ingress_admission_order, packet.payload.work_id)
       /\ runtime_ingress_content_shape' = MapSet(runtime_ingress_content_shape, packet.payload.work_id, packet.payload.content_shape)
       /\ runtime_ingress_request_id' = MapSet(runtime_ingress_request_id, packet.payload.work_id, packet.payload.request_id)
       /\ runtime_ingress_reservation_key' = MapSet(runtime_ingress_reservation_key, packet.payload.work_id, packet.payload.reservation_key)
       /\ runtime_ingress_policy_snapshot' = MapSet(runtime_ingress_policy_snapshot, packet.payload.work_id, packet.payload.policy)
       /\ runtime_ingress_lifecycle' = MapSet(runtime_ingress_lifecycle, packet.payload.work_id, "Consumed")
       /\ runtime_ingress_terminal_outcome' = MapSet(runtime_ingress_terminal_outcome, packet.payload.work_id, Some("Consumed"))
       /\ runtime_ingress_last_run' = MapSet(runtime_ingress_last_run, packet.payload.work_id, None)
       /\ runtime_ingress_last_boundary_sequence' = MapSet(runtime_ingress_last_boundary_sequence, packet.payload.work_id, None)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_handling_mode, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressAccepted", payload |-> [work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitConsumedOnAccept"], [machine |-> "runtime_ingress", variant |-> "InputLifecycleNotice", payload |-> [new_state |-> "Consumed", work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitConsumedOnAccept"], [machine |-> "runtime_ingress", variant |-> "CompletionResolved", payload |-> [outcome |-> "Consumed", work_id |-> packet.payload.work_id], effect_id |-> (model_step_count + 1), source_transition |-> "AdmitConsumedOnAccept"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "AdmitConsumedOnAccept", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "StageDrainSnapshot"
       /\ packet.payload.run_id = arg_run_id
       /\ packet.payload.contributing_work_ids = arg_contributing_work_ids
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = None)
       /\ (Len(packet.payload.contributing_work_ids) > 0)
       /\ (((Len(runtime_ingress_steer_queue) > 0) /\ StartsWith(runtime_ingress_steer_queue, packet.payload.contributing_work_ids)) \/ ((Len(runtime_ingress_steer_queue) = 0) /\ StartsWith(runtime_ingress_queue, packet.payload.contributing_work_ids)))
       /\ (\A work_id \in SeqElements(packet.payload.contributing_work_ids) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Queued"))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_StageDrainSnapshotFromActive_ForEach0_lifecycle(runtime_ingress_lifecycle, packet.payload.contributing_work_ids, packet.payload.run_id)
       /\ runtime_ingress_queue' = IF (Len(runtime_ingress_steer_queue) > 0) THEN runtime_ingress_queue ELSE SeqRemoveAll(runtime_ingress_queue, packet.payload.contributing_work_ids)
       /\ runtime_ingress_steer_queue' = IF (Len(runtime_ingress_steer_queue) > 0) THEN SeqRemoveAll(runtime_ingress_steer_queue, packet.payload.contributing_work_ids) ELSE runtime_ingress_steer_queue
       /\ runtime_ingress_current_run' = Some(packet.payload.run_id)
       /\ runtime_ingress_current_run_contributors' = packet.payload.contributing_work_ids
       /\ runtime_ingress_last_run' = runtime_ingress_StageDrainSnapshotFromActive_ForEach0_last_run(runtime_ingress_last_run, packet.payload.contributing_work_ids, packet.payload.run_id)
       /\ runtime_ingress_wake_requested' = FALSE
       /\ runtime_ingress_process_requested' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_boundary_sequence, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], source_kind |-> "route", source_route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", source_effect |-> "ReadyForRun", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], source_kind |-> "route", source_route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", source_effect |-> "ReadyForRun", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", effect |-> "ReadyForRun", target_machine |-> "runtime_control", target_input |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "StageDrainSnapshotFromActive"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "ReadyForRun", payload |-> [contributing_work_ids |-> packet.payload.contributing_work_ids, run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "StageDrainSnapshotFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "StageDrainSnapshotFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "StageDrainSnapshot"
       /\ packet.payload.run_id = arg_run_id
       /\ packet.payload.contributing_work_ids = arg_contributing_work_ids
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = None)
       /\ (Len(packet.payload.contributing_work_ids) > 0)
       /\ (((Len(runtime_ingress_steer_queue) > 0) /\ StartsWith(runtime_ingress_steer_queue, packet.payload.contributing_work_ids)) \/ ((Len(runtime_ingress_steer_queue) = 0) /\ StartsWith(runtime_ingress_queue, packet.payload.contributing_work_ids)))
       /\ (\A work_id \in SeqElements(packet.payload.contributing_work_ids) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Queued"))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_lifecycle(runtime_ingress_lifecycle, packet.payload.contributing_work_ids, packet.payload.run_id)
       /\ runtime_ingress_queue' = IF (Len(runtime_ingress_steer_queue) > 0) THEN runtime_ingress_queue ELSE SeqRemoveAll(runtime_ingress_queue, packet.payload.contributing_work_ids)
       /\ runtime_ingress_steer_queue' = IF (Len(runtime_ingress_steer_queue) > 0) THEN SeqRemoveAll(runtime_ingress_steer_queue, packet.payload.contributing_work_ids) ELSE runtime_ingress_steer_queue
       /\ runtime_ingress_current_run' = Some(packet.payload.run_id)
       /\ runtime_ingress_current_run_contributors' = packet.payload.contributing_work_ids
       /\ runtime_ingress_last_run' = runtime_ingress_StageDrainSnapshotFromRetired_ForEach1_last_run(runtime_ingress_last_run, packet.payload.contributing_work_ids, packet.payload.run_id)
       /\ runtime_ingress_wake_requested' = FALSE
       /\ runtime_ingress_process_requested' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_boundary_sequence, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = AppendIfMissing(SeqRemove(pending_inputs, packet), [machine |-> "runtime_control", variant |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], source_kind |-> "route", source_route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", source_effect |-> "ReadyForRun", effect_id |-> (model_step_count + 1)])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], source_kind |-> "route", source_route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", source_effect |-> "ReadyForRun", effect_id |-> (model_step_count + 1)]}
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes \cup { [route |-> "peer_ingress_ready_begins_run", source_machine |-> "runtime_ingress", effect |-> "ReadyForRun", target_machine |-> "runtime_control", target_input |-> "BeginRun", payload |-> [run_id |-> packet.payload.run_id], actor |-> "control_plane", effect_id |-> (model_step_count + 1), source_transition |-> "StageDrainSnapshotFromRetired"] }
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "ReadyForRun", payload |-> [contributing_work_ids |-> packet.payload.contributing_work_ids, run_id |-> packet.payload.run_id], effect_id |-> (model_step_count + 1), source_transition |-> "StageDrainSnapshotFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "StageDrainSnapshotFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "BoundaryApplied"
       /\ packet.payload.run_id = arg_run_id
       /\ packet.payload.boundary_sequence = arg_boundary_sequence
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ (\A work_id \in SeqElements(runtime_ingress_current_run_contributors) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Staged"))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_BoundaryAppliedFromActive_ForEach2_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, packet.payload.boundary_sequence)
       /\ runtime_ingress_last_boundary_sequence' = runtime_ingress_BoundaryAppliedFromActive_ForEach2_last_boundary_sequence(runtime_ingress_last_boundary_sequence, runtime_ingress_current_run_contributors, packet.payload.boundary_sequence)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "ContributorsPendingConsumption", kind |-> "BoundaryApplied"], effect_id |-> (model_step_count + 1), source_transition |-> "BoundaryAppliedFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "BoundaryAppliedFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "BoundaryApplied"
       /\ packet.payload.run_id = arg_run_id
       /\ packet.payload.boundary_sequence = arg_boundary_sequence
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ (\A work_id \in SeqElements(runtime_ingress_current_run_contributors) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Staged"))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_BoundaryAppliedFromRetired_ForEach3_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, packet.payload.boundary_sequence)
       /\ runtime_ingress_last_boundary_sequence' = runtime_ingress_BoundaryAppliedFromRetired_ForEach3_last_boundary_sequence(runtime_ingress_last_boundary_sequence, runtime_ingress_current_run_contributors, packet.payload.boundary_sequence)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "ContributorsPendingConsumption", kind |-> "BoundaryApplied"], effect_id |-> (model_step_count + 1), source_transition |-> "BoundaryAppliedFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "BoundaryAppliedFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunCompletedFromActive(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ (\A work_id \in SeqElements(runtime_ingress_current_run_contributors) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "AppliedPendingConsumption"))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunCompletedFromActive_ForEach4_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_RunCompletedFromActive_ForEach4_terminal_outcome(runtime_ingress_terminal_outcome, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "ContributorsConsumed", kind |-> "RunCompleted"], effect_id |-> (model_step_count + 1), source_transition |-> "RunCompletedFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunCompletedFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunCompletedFromRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunCompleted"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ (\A work_id \in SeqElements(runtime_ingress_current_run_contributors) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "AppliedPendingConsumption"))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunCompletedFromRetired_ForEach5_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_RunCompletedFromRetired_ForEach5_terminal_outcome(runtime_ingress_terminal_outcome, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "ContributorsConsumed", kind |-> "RunCompleted"], effect_id |-> (model_step_count + 1), source_transition |-> "RunCompletedFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunCompletedFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunFailedFromActive(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode)
       /\ runtime_ingress_queue' = runtime_ingress_RunFailedFromActive_ForEach6_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_steer_queue' = runtime_ingress_RunFailedFromActive_ForEach6_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF ((Len(runtime_ingress_RunFailedFromActive_ForEach6_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0) \/ (Len(runtime_ingress_RunFailedFromActive_ForEach6_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromActive_ForEach6_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0)) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "StagedRolledBack", kind |-> "RunFailed"], effect_id |-> (model_step_count + 1), source_transition |-> "RunFailedFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunFailedFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunFailedFromRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunFailed"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode)
       /\ runtime_ingress_queue' = runtime_ingress_RunFailedFromRetired_ForEach7_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_steer_queue' = runtime_ingress_RunFailedFromRetired_ForEach7_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF ((Len(runtime_ingress_RunFailedFromRetired_ForEach7_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0) \/ (Len(runtime_ingress_RunFailedFromRetired_ForEach7_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunFailedFromRetired_ForEach7_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0)) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "StagedRolledBack", kind |-> "RunFailed"], effect_id |-> (model_step_count + 1), source_transition |-> "RunFailedFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunFailedFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunCancelledFromActive(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode)
       /\ runtime_ingress_queue' = runtime_ingress_RunCancelledFromActive_ForEach8_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_steer_queue' = runtime_ingress_RunCancelledFromActive_ForEach8_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF ((Len(runtime_ingress_RunCancelledFromActive_ForEach8_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0) \/ (Len(runtime_ingress_RunCancelledFromActive_ForEach8_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromActive_ForEach8_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0)) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "StagedRolledBack", kind |-> "RunCancelled"], effect_id |-> (model_step_count + 1), source_transition |-> "RunCancelledFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunCancelledFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RunCancelledFromRetired(arg_run_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "RunCancelled"
       /\ packet.payload.run_id = arg_run_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = Some(packet.payload.run_id))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode)
       /\ runtime_ingress_queue' = runtime_ingress_RunCancelledFromRetired_ForEach9_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_steer_queue' = runtime_ingress_RunCancelledFromRetired_ForEach9_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF ((Len(runtime_ingress_RunCancelledFromRetired_ForEach9_queue(runtime_ingress_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0) \/ (Len(runtime_ingress_RunCancelledFromRetired_ForEach9_steer_queue(runtime_ingress_steer_queue, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode, runtime_ingress_RunCancelledFromRetired_ForEach9_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors, runtime_ingress_handling_mode))) > 0)) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "StagedRolledBack", kind |-> "RunCancelled"], effect_id |-> (model_step_count + 1), source_transition |-> "RunCancelledFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RunCancelledFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "SupersedeQueuedInput"
       /\ packet.payload.new_work_id = arg_new_work_id
       /\ packet.payload.old_work_id = arg_old_work_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (packet.payload.new_work_id \in runtime_ingress_admitted_inputs)
       /\ ((IF packet.payload.old_work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[packet.payload.old_work_id] ELSE "None") = "Queued")
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = MapSet(runtime_ingress_lifecycle, packet.payload.old_work_id, "Superseded")
       /\ runtime_ingress_terminal_outcome' = MapSet(runtime_ingress_terminal_outcome, packet.payload.old_work_id, Some("Superseded"))
       /\ runtime_ingress_queue' = SeqRemove(runtime_ingress_queue, packet.payload.old_work_id)
       /\ runtime_ingress_steer_queue' = SeqRemove(runtime_ingress_steer_queue, packet.payload.old_work_id)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "InputLifecycleNotice", payload |-> [new_state |-> "Superseded", work_id |-> packet.payload.old_work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SupersedeQueuedInputFromActive"], [machine |-> "runtime_ingress", variant |-> "CompletionResolved", payload |-> [outcome |-> "Superseded", work_id |-> packet.payload.old_work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SupersedeQueuedInputFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "SupersedeQueuedInputFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "SupersedeQueuedInput"
       /\ packet.payload.new_work_id = arg_new_work_id
       /\ packet.payload.old_work_id = arg_old_work_id
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (packet.payload.new_work_id \in runtime_ingress_admitted_inputs)
       /\ ((IF packet.payload.old_work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[packet.payload.old_work_id] ELSE "None") = "Queued")
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = MapSet(runtime_ingress_lifecycle, packet.payload.old_work_id, "Superseded")
       /\ runtime_ingress_terminal_outcome' = MapSet(runtime_ingress_terminal_outcome, packet.payload.old_work_id, Some("Superseded"))
       /\ runtime_ingress_queue' = SeqRemove(runtime_ingress_queue, packet.payload.old_work_id)
       /\ runtime_ingress_steer_queue' = SeqRemove(runtime_ingress_steer_queue, packet.payload.old_work_id)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "InputLifecycleNotice", payload |-> [new_state |-> "Superseded", work_id |-> packet.payload.old_work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SupersedeQueuedInputFromRetired"], [machine |-> "runtime_ingress", variant |-> "CompletionResolved", payload |-> [outcome |-> "Superseded", work_id |-> packet.payload.old_work_id], effect_id |-> (model_step_count + 1), source_transition |-> "SupersedeQueuedInputFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "SupersedeQueuedInputFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "CoalesceQueuedInputs"
       /\ packet.payload.aggregate_work_id = arg_aggregate_work_id
       /\ packet.payload.source_work_ids = arg_source_work_ids
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (packet.payload.aggregate_work_id \in runtime_ingress_admitted_inputs)
       /\ (Len(packet.payload.source_work_ids) > 0)
       /\ (\A work_id \in SeqElements(packet.payload.source_work_ids) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Queued"))
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_lifecycle(runtime_ingress_lifecycle, packet.payload.source_work_ids)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_CoalesceQueuedInputsFromActive_ForEach10_terminal_outcome(runtime_ingress_terminal_outcome, packet.payload.source_work_ids)
       /\ runtime_ingress_queue' = SeqRemoveAll(runtime_ingress_queue, packet.payload.source_work_ids)
       /\ runtime_ingress_steer_queue' = SeqRemoveAll(runtime_ingress_steer_queue, packet.payload.source_work_ids)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "SourcesCoalescedIntoAggregate", kind |-> "CoalesceQueuedInputs"], effect_id |-> (model_step_count + 1), source_transition |-> "CoalesceQueuedInputsFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "CoalesceQueuedInputsFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "CoalesceQueuedInputs"
       /\ packet.payload.aggregate_work_id = arg_aggregate_work_id
       /\ packet.payload.source_work_ids = arg_source_work_ids
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (packet.payload.aggregate_work_id \in runtime_ingress_admitted_inputs)
       /\ (Len(packet.payload.source_work_ids) > 0)
       /\ (\A work_id \in SeqElements(packet.payload.source_work_ids) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Queued"))
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_lifecycle(runtime_ingress_lifecycle, packet.payload.source_work_ids)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_CoalesceQueuedInputsFromRetired_ForEach11_terminal_outcome(runtime_ingress_terminal_outcome, packet.payload.source_work_ids)
       /\ runtime_ingress_queue' = SeqRemoveAll(runtime_ingress_queue, packet.payload.source_work_ids)
       /\ runtime_ingress_steer_queue' = SeqRemoveAll(runtime_ingress_steer_queue, packet.payload.source_work_ids)
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "SourcesCoalescedIntoAggregate", kind |-> "CoalesceQueuedInputs"], effect_id |-> (model_step_count + 1), source_transition |-> "CoalesceQueuedInputsFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "CoalesceQueuedInputsFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_Retire ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Retire"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ runtime_ingress_phase' = "Retired"
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "QueuePreserved", kind |-> "Retire"], effect_id |-> (model_step_count + 1), source_transition |-> "Retire"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "Retire", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_ResetFromActive ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Reset"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ (runtime_ingress_current_run = None)
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_ResetFromActive_ForEach12_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_ResetFromActive_ForEach12_terminal_outcome(runtime_ingress_terminal_outcome, DOMAIN runtime_ingress_lifecycle, runtime_ingress_ResetFromActive_ForEach12_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome))
       /\ runtime_ingress_queue' = <<>>
       /\ runtime_ingress_steer_queue' = <<>>
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = FALSE
       /\ runtime_ingress_process_requested' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "NonTerminalInputsAbandoned", kind |-> "Reset"], effect_id |-> (model_step_count + 1), source_transition |-> "ResetFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "ResetFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_ResetFromRetired ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Reset"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ (runtime_ingress_current_run = None)
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_ResetFromRetired_ForEach13_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_ResetFromRetired_ForEach13_terminal_outcome(runtime_ingress_terminal_outcome, DOMAIN runtime_ingress_lifecycle, runtime_ingress_ResetFromRetired_ForEach13_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome))
       /\ runtime_ingress_queue' = <<>>
       /\ runtime_ingress_steer_queue' = <<>>
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = FALSE
       /\ runtime_ingress_process_requested' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "NonTerminalInputsAbandoned", kind |-> "Reset"], effect_id |-> (model_step_count + 1), source_transition |-> "ResetFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "ResetFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_Destroy ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Destroy"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active" \/ runtime_ingress_phase = "Retired"
       /\ runtime_ingress_phase' = "Destroyed"
       /\ runtime_ingress_lifecycle' = runtime_ingress_Destroy_ForEach14_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome)
       /\ runtime_ingress_terminal_outcome' = runtime_ingress_Destroy_ForEach14_terminal_outcome(runtime_ingress_terminal_outcome, DOMAIN runtime_ingress_lifecycle, runtime_ingress_Destroy_ForEach14_lifecycle(runtime_ingress_lifecycle, DOMAIN runtime_ingress_lifecycle, runtime_ingress_terminal_outcome))
       /\ runtime_ingress_queue' = <<>>
       /\ runtime_ingress_steer_queue' = <<>>
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = FALSE
       /\ runtime_ingress_process_requested' = FALSE
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "IngressDestroyed", kind |-> "Destroy"], effect_id |-> (model_step_count + 1), source_transition |-> "Destroy"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "Destroy", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Destroyed"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RecoverFromActive ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Recover"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RecoverFromActive_ForEach15_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_queue' = IF (Len(runtime_ingress_current_run_contributors) > 0) THEN (runtime_ingress_current_run_contributors \o runtime_ingress_queue) ELSE runtime_ingress_queue
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF (Len(runtime_ingress_current_run_contributors) > 0) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_steer_queue, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "RecoveryAppliedToCurrentRun", kind |-> "Recover"], effect_id |-> (model_step_count + 1), source_transition |-> "RecoverFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RecoverFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_RecoverFromRetired ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "Recover"
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_lifecycle' = runtime_ingress_RecoverFromRetired_ForEach16_lifecycle(runtime_ingress_lifecycle, runtime_ingress_current_run_contributors)
       /\ runtime_ingress_queue' = IF (Len(runtime_ingress_current_run_contributors) > 0) THEN (runtime_ingress_current_run_contributors \o runtime_ingress_queue) ELSE runtime_ingress_queue
       /\ runtime_ingress_current_run' = None
       /\ runtime_ingress_current_run_contributors' = <<>>
       /\ runtime_ingress_wake_requested' = IF (Len(runtime_ingress_current_run_contributors) > 0) THEN TRUE ELSE runtime_ingress_wake_requested
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_terminal_outcome, runtime_ingress_steer_queue, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "RecoveryAppliedToCurrentRun", kind |-> "Recover"], effect_id |-> (model_step_count + 1), source_transition |-> "RecoverFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "RecoverFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "SetSilentIntentOverrides"
       /\ packet.payload.intents = arg_intents
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Active"
       /\ runtime_ingress_phase' = "Active"
       /\ runtime_ingress_silent_intent_overrides' = packet.payload.intents
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "Updated", kind |-> "SilentIntentOverrides"], effect_id |-> (model_step_count + 1), source_transition |-> "SetSilentIntentOverridesFromActive"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "SetSilentIntentOverridesFromActive", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Active"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents) ==
    /\ \E packet \in SeqElements(pending_inputs) :
       /\ packet.machine = "runtime_ingress"
       /\ packet.variant = "SetSilentIntentOverrides"
       /\ packet.payload.intents = arg_intents
       /\ ~HigherPriorityReady("ordinary_ingress")
       /\ runtime_ingress_phase = "Retired"
       /\ runtime_ingress_phase' = "Retired"
       /\ runtime_ingress_silent_intent_overrides' = packet.payload.intents
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, witness_current_script_input, witness_remaining_script_inputs >>
       /\ pending_inputs' = SeqRemove(pending_inputs, packet)
       /\ observed_inputs' = observed_inputs
       /\ pending_routes' = pending_routes
       /\ delivered_routes' = delivered_routes
       /\ emitted_effects' = emitted_effects \cup { [machine |-> "runtime_ingress", variant |-> "IngressNotice", payload |-> [detail |-> "Updated", kind |-> "SilentIntentOverrides"], effect_id |-> (model_step_count + 1), source_transition |-> "SetSilentIntentOverridesFromRetired"] }
       /\ observed_transitions' = observed_transitions \cup {[machine |-> "runtime_ingress", transition |-> "SetSilentIntentOverridesFromRetired", actor |-> "ordinary_ingress", step |-> (model_step_count + 1), from_phase |-> runtime_ingress_phase, to_phase |-> "Retired"]}
       /\ model_step_count' = model_step_count + 1


runtime_ingress_queue_entries_are_queued == (\A work_id \in SeqElements(runtime_ingress_queue) : ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") = "Queued"))
runtime_ingress_steer_entries_are_queued == (\A work_id \in SeqElements(runtime_ingress_steer_queue) : (work_id \in DOMAIN runtime_ingress_lifecycle))
runtime_ingress_pending_inputs_preserve_content_shape == ((\A work_id \in SeqElements(runtime_ingress_queue) : (work_id \in DOMAIN runtime_ingress_content_shape)) /\ (\A work_id \in SeqElements(runtime_ingress_steer_queue) : (work_id \in DOMAIN runtime_ingress_content_shape)))
runtime_ingress_admitted_inputs_preserve_correlation_slots == (\A work_id \in runtime_ingress_admitted_inputs : ((work_id \in DOMAIN runtime_ingress_request_id) /\ (work_id \in DOMAIN runtime_ingress_reservation_key)))
runtime_ingress_queue_entries_preserve_handling_mode == (\A work_id \in SeqElements(runtime_ingress_queue) : ((IF work_id \in DOMAIN runtime_ingress_handling_mode THEN runtime_ingress_handling_mode[work_id] ELSE "None") = "Queue"))
runtime_ingress_steer_entries_preserve_handling_mode == (\A work_id \in SeqElements(runtime_ingress_steer_queue) : ((IF work_id \in DOMAIN runtime_ingress_handling_mode THEN runtime_ingress_handling_mode[work_id] ELSE "None") = "Steer"))
runtime_ingress_pending_queues_do_not_overlap == (\A work_id \in SeqElements(runtime_ingress_steer_queue) : ~((work_id \in SeqElements(runtime_ingress_queue))))
runtime_ingress_terminal_inputs_do_not_appear_in_queue == ((\A work_id \in SeqElements(runtime_ingress_queue) : (((IF work_id \in DOMAIN runtime_ingress_terminal_outcome THEN runtime_ingress_terminal_outcome[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Abandoned"))) /\ (\A work_id \in SeqElements(runtime_ingress_steer_queue) : (((IF work_id \in DOMAIN runtime_ingress_terminal_outcome THEN runtime_ingress_terminal_outcome[work_id] ELSE None) = None) /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Consumed") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Superseded") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Coalesced") /\ ((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "Abandoned"))))
runtime_ingress_current_run_matches_contributor_presence == ((runtime_ingress_current_run = None) = (Len(runtime_ingress_current_run_contributors) = 0))
runtime_ingress_staged_contributors_are_not_queued == (\A work_id \in SeqElements(runtime_ingress_current_run_contributors) : (~((work_id \in SeqElements(runtime_ingress_queue))) /\ ~((work_id \in SeqElements(runtime_ingress_steer_queue)))))
runtime_ingress_applied_pending_consumption_has_last_run == (\A work_id \in runtime_ingress_admitted_inputs : (((IF work_id \in DOMAIN runtime_ingress_lifecycle THEN runtime_ingress_lifecycle[work_id] ELSE "None") # "AppliedPendingConsumption") \/ ((IF work_id \in DOMAIN runtime_ingress_last_run THEN runtime_ingress_last_run[work_id] ELSE None) # None)))

Inject_control_initialize ==
    /\ ~([machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "control_initialize", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0] \in SeqElements(pending_inputs))
    /\ pending_inputs' = Append(pending_inputs, [machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "control_initialize", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0])
    /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "Initialize", payload |-> [tag |-> "unit"], source_kind |-> "entry", source_route |-> "control_initialize", source_machine |-> "external_entry", source_effect |-> "Initialize", effect_id |-> 0]}
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

Inject_classify_peer_envelope(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message) ==
    /\ ~([machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> arg_dismiss_message, fallback_sender_name |-> arg_fallback_sender_name, handling_mode |-> arg_handling_mode, handling_mode_present |-> arg_handling_mode_present, intent |-> arg_intent, kind |-> arg_kind, lifecycle_peer |-> arg_lifecycle_peer, lifecycle_peer_present |-> arg_lifecycle_peer_present, raw_item_id |-> arg_raw_item_id, require_peer_auth |-> arg_require_peer_auth, sender_name |-> arg_sender_name, sender_name_known |-> arg_sender_name_known, silent_intent |-> arg_silent_intent], source_kind |-> "entry", source_route |-> "classify_peer_envelope", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0] \in SeqElements(pending_inputs))
    /\ pending_inputs' = Append(pending_inputs, [machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> arg_dismiss_message, fallback_sender_name |-> arg_fallback_sender_name, handling_mode |-> arg_handling_mode, handling_mode_present |-> arg_handling_mode_present, intent |-> arg_intent, kind |-> arg_kind, lifecycle_peer |-> arg_lifecycle_peer, lifecycle_peer_present |-> arg_lifecycle_peer_present, raw_item_id |-> arg_raw_item_id, require_peer_auth |-> arg_require_peer_auth, sender_name |-> arg_sender_name, sender_name_known |-> arg_sender_name_known, silent_intent |-> arg_silent_intent], source_kind |-> "entry", source_route |-> "classify_peer_envelope", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0])
    /\ observed_inputs' = observed_inputs \cup {[machine |-> "peer_comms", variant |-> "ClassifyExternalEnvelope", payload |-> [dismiss_message |-> arg_dismiss_message, fallback_sender_name |-> arg_fallback_sender_name, handling_mode |-> arg_handling_mode, handling_mode_present |-> arg_handling_mode_present, intent |-> arg_intent, kind |-> arg_kind, lifecycle_peer |-> arg_lifecycle_peer, lifecycle_peer_present |-> arg_lifecycle_peer_present, raw_item_id |-> arg_raw_item_id, require_peer_auth |-> arg_require_peer_auth, sender_name |-> arg_sender_name, sender_name_known |-> arg_sender_name_known, silent_intent |-> arg_silent_intent], source_kind |-> "entry", source_route |-> "classify_peer_envelope", source_machine |-> "external_entry", source_effect |-> "ClassifyExternalEnvelope", effect_id |-> 0]}
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

Inject_classify_plain_event(arg_raw_item_id, arg_source_name, arg_handling_mode) ==
    /\ ~([machine |-> "peer_comms", variant |-> "ClassifyPlainEvent", payload |-> [handling_mode |-> arg_handling_mode, raw_item_id |-> arg_raw_item_id, source_name |-> arg_source_name], source_kind |-> "entry", source_route |-> "classify_plain_event", source_machine |-> "external_entry", source_effect |-> "ClassifyPlainEvent", effect_id |-> 0] \in SeqElements(pending_inputs))
    /\ pending_inputs' = Append(pending_inputs, [machine |-> "peer_comms", variant |-> "ClassifyPlainEvent", payload |-> [handling_mode |-> arg_handling_mode, raw_item_id |-> arg_raw_item_id, source_name |-> arg_source_name], source_kind |-> "entry", source_route |-> "classify_plain_event", source_machine |-> "external_entry", source_effect |-> "ClassifyPlainEvent", effect_id |-> 0])
    /\ observed_inputs' = observed_inputs \cup {[machine |-> "peer_comms", variant |-> "ClassifyPlainEvent", payload |-> [handling_mode |-> arg_handling_mode, raw_item_id |-> arg_raw_item_id, source_name |-> arg_source_name], source_kind |-> "entry", source_route |-> "classify_plain_event", source_machine |-> "external_entry", source_effect |-> "ClassifyPlainEvent", effect_id |-> 0]}
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

Inject_runtime_admission_accepted(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect) ==
    /\ ~([machine |-> "runtime_control", variant |-> "AdmissionAccepted", payload |-> [admission_effect |-> arg_admission_effect, content_shape |-> arg_content_shape, handling_mode |-> arg_handling_mode, request_id |-> arg_request_id, reservation_key |-> arg_reservation_key, work_id |-> arg_work_id], source_kind |-> "entry", source_route |-> "runtime_admission_accepted", source_machine |-> "external_entry", source_effect |-> "AdmissionAccepted", effect_id |-> 0] \in SeqElements(pending_inputs))
    /\ pending_inputs' = Append(pending_inputs, [machine |-> "runtime_control", variant |-> "AdmissionAccepted", payload |-> [admission_effect |-> arg_admission_effect, content_shape |-> arg_content_shape, handling_mode |-> arg_handling_mode, request_id |-> arg_request_id, reservation_key |-> arg_reservation_key, work_id |-> arg_work_id], source_kind |-> "entry", source_route |-> "runtime_admission_accepted", source_machine |-> "external_entry", source_effect |-> "AdmissionAccepted", effect_id |-> 0])
    /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_control", variant |-> "AdmissionAccepted", payload |-> [admission_effect |-> arg_admission_effect, content_shape |-> arg_content_shape, handling_mode |-> arg_handling_mode, request_id |-> arg_request_id, reservation_key |-> arg_reservation_key, work_id |-> arg_work_id], source_kind |-> "entry", source_route |-> "runtime_admission_accepted", source_machine |-> "external_entry", source_effect |-> "AdmissionAccepted", effect_id |-> 0]}
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

Inject_ingress_stage_drain_snapshot(arg_run_id, arg_contributing_work_ids) ==
    /\ ~([machine |-> "runtime_ingress", variant |-> "StageDrainSnapshot", payload |-> [contributing_work_ids |-> arg_contributing_work_ids, run_id |-> arg_run_id], source_kind |-> "entry", source_route |-> "ingress_stage_drain_snapshot", source_machine |-> "external_entry", source_effect |-> "StageDrainSnapshot", effect_id |-> 0] \in SeqElements(pending_inputs))
    /\ pending_inputs' = Append(pending_inputs, [machine |-> "runtime_ingress", variant |-> "StageDrainSnapshot", payload |-> [contributing_work_ids |-> arg_contributing_work_ids, run_id |-> arg_run_id], source_kind |-> "entry", source_route |-> "ingress_stage_drain_snapshot", source_machine |-> "external_entry", source_effect |-> "StageDrainSnapshot", effect_id |-> 0])
    /\ observed_inputs' = observed_inputs \cup {[machine |-> "runtime_ingress", variant |-> "StageDrainSnapshot", payload |-> [contributing_work_ids |-> arg_contributing_work_ids, run_id |-> arg_run_id], source_kind |-> "entry", source_route |-> "ingress_stage_drain_snapshot", source_machine |-> "external_entry", source_effect |-> "StageDrainSnapshot", effect_id |-> 0]}
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

DeliverQueuedRoute ==
    /\ Len(pending_routes) > 0
    /\ LET route == Head(pending_routes) IN
       /\ pending_routes' = Tail(pending_routes)
       /\ delivered_routes' = delivered_routes \cup {route}
       /\ model_step_count' = model_step_count + 1
       /\ pending_inputs' = AppendIfMissing(pending_inputs, [machine |-> route.target_machine, variant |-> route.target_input, payload |-> route.payload, source_kind |-> "route", source_route |-> route.route, source_machine |-> route.source_machine, source_effect |-> route.effect, effect_id |-> route.effect_id])
       /\ observed_inputs' = observed_inputs \cup {[machine |-> route.target_machine, variant |-> route.target_input, payload |-> route.payload, source_kind |-> "route", source_route |-> route.route, source_machine |-> route.source_machine, source_effect |-> route.effect, effect_id |-> route.effect_id]}
       /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, emitted_effects, observed_transitions, witness_current_script_input, witness_remaining_script_inputs >>

QuiescentStutter ==
    /\ Len(pending_routes) = 0
    /\ Len(pending_inputs) = 0
    /\ UNCHANGED vars

WitnessInjectNext_trusted_peer_enters_runtime ==
    /\ witness_current_script_input # None
    /\ ~(witness_current_script_input \in SeqElements(pending_inputs))
    /\ Len(pending_inputs) = 0
    /\ Len(pending_routes) = 0
    /\ Len(witness_remaining_script_inputs) > 0
    /\ pending_inputs' = Append(pending_inputs, Head(witness_remaining_script_inputs))
    /\ observed_inputs' = observed_inputs \cup {Head(witness_remaining_script_inputs)}
    /\ witness_current_script_input' = Head(witness_remaining_script_inputs)
    /\ witness_remaining_script_inputs' = Tail(witness_remaining_script_inputs)
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions >>

WitnessInjectNext_admitted_peer_work_enters_ingress ==
    /\ witness_current_script_input # None
    /\ ~(witness_current_script_input \in SeqElements(pending_inputs))
    /\ Len(pending_inputs) = 0
    /\ Len(pending_routes) = 0
    /\ Len(witness_remaining_script_inputs) > 0
    /\ pending_inputs' = Append(pending_inputs, Head(witness_remaining_script_inputs))
    /\ observed_inputs' = observed_inputs \cup {Head(witness_remaining_script_inputs)}
    /\ witness_current_script_input' = Head(witness_remaining_script_inputs)
    /\ witness_remaining_script_inputs' = Tail(witness_remaining_script_inputs)
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions >>

WitnessInjectNext_peer_ingress_ready_begins_run ==
    /\ witness_current_script_input # None
    /\ ~(witness_current_script_input \in SeqElements(pending_inputs))
    /\ Len(pending_inputs) = 0
    /\ Len(pending_routes) = 0
    /\ Len(witness_remaining_script_inputs) > 0
    /\ pending_inputs' = Append(pending_inputs, Head(witness_remaining_script_inputs))
    /\ observed_inputs' = observed_inputs \cup {Head(witness_remaining_script_inputs)}
    /\ witness_current_script_input' = Head(witness_remaining_script_inputs)
    /\ witness_remaining_script_inputs' = Tail(witness_remaining_script_inputs)
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions >>

WitnessInjectNext_control_preempts_peer_delivery ==
    /\ witness_current_script_input # None
    /\ ~(witness_current_script_input \in SeqElements(pending_inputs))
    /\ Len(pending_inputs) = 0
    /\ Len(pending_routes) = 0
    /\ Len(witness_remaining_script_inputs) > 0
    /\ pending_inputs' = Append(pending_inputs, Head(witness_remaining_script_inputs))
    /\ observed_inputs' = observed_inputs \cup {Head(witness_remaining_script_inputs)}
    /\ witness_current_script_input' = Head(witness_remaining_script_inputs)
    /\ witness_remaining_script_inputs' = Tail(witness_remaining_script_inputs)
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << peer_comms_phase, runtime_control_phase, runtime_control_current_run_id, runtime_control_pre_run_state, runtime_control_wake_pending, runtime_control_process_pending, runtime_ingress_phase, runtime_ingress_admitted_inputs, runtime_ingress_admission_order, runtime_ingress_content_shape, runtime_ingress_request_id, runtime_ingress_reservation_key, runtime_ingress_policy_snapshot, runtime_ingress_handling_mode, runtime_ingress_lifecycle, runtime_ingress_terminal_outcome, runtime_ingress_queue, runtime_ingress_steer_queue, runtime_ingress_current_run, runtime_ingress_current_run_contributors, runtime_ingress_last_run, runtime_ingress_last_boundary_sequence, runtime_ingress_wake_requested, runtime_ingress_process_requested, runtime_ingress_silent_intent_overrides, pending_routes, delivered_routes, emitted_effects, observed_transitions >>

CoreNext ==
    \/ DeliverQueuedRoute
    \/ \E arg_require_peer_auth \in BOOLEAN : \E arg_raw_item_id \in RawItemIdValues : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode)
    \/ runtime_control_Initialize
    \/ runtime_control_AttachFromIdle
    \/ runtime_control_DetachToIdle
    \/ \E arg_run_id \in RunIdValues : runtime_control_BeginRunFromIdle(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_BeginRunFromAttached(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRecovering(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCompletedToIdle(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCompletedToAttached(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCompletedToRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunFailedToIdle(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunFailedToAttached(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunFailedToRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCancelledToIdle(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCancelledToAttached(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCancelledToRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCompletedFromRetiredInFlight(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunFailedFromRetiredInFlight(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_control_RunCancelledFromRetiredInFlight(arg_run_id)
    \/ runtime_control_RecoverRequestedFromIdle
    \/ runtime_control_RecoverRequestedFromRunning
    \/ runtime_control_RecoverRequestedFromAttached
    \/ runtime_control_RecoverySucceeded
    \/ runtime_control_RetireRequestedFromIdle
    \/ runtime_control_RetireRequestedFromRunning
    \/ runtime_control_RetireRequestedFromAttached
    \/ runtime_control_ResetRequested
    \/ runtime_control_StopRequested
    \/ runtime_control_DestroyRequested
    \/ runtime_control_ResumeRequested
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason)
    \/ \E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason)
    \/ \E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason)
    \/ \E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id)
    \/ \E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id)
    \/ \E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id)
    \/ runtime_control_ExternalToolDeltaReceivedIdle
    \/ runtime_control_ExternalToolDeltaReceivedRunning
    \/ runtime_control_ExternalToolDeltaReceivedRecovering
    \/ runtime_control_ExternalToolDeltaReceivedRetired
    \/ runtime_control_ExternalToolDeltaReceivedAttached
    \/ runtime_control_RecycleRequestedFromRetired
    \/ runtime_control_RecycleRequestedFromIdle
    \/ runtime_control_RecycleRequestedFromAttached
    \/ runtime_control_RecycleSucceeded
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy)
    \/ \E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids)
    \/ \E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids)
    \/ \E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence)
    \/ \E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromActive(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromActive(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromRetired(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromActive(arg_run_id)
    \/ \E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromRetired(arg_run_id)
    \/ \E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id)
    \/ \E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id)
    \/ \E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids)
    \/ \E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids)
    \/ runtime_ingress_Retire
    \/ runtime_ingress_ResetFromActive
    \/ runtime_ingress_ResetFromRetired
    \/ runtime_ingress_Destroy
    \/ runtime_ingress_RecoverFromActive
    \/ runtime_ingress_RecoverFromRetired
    \/ \E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents)
    \/ \E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents)
    \/ QuiescentStutter

InjectNext ==
    \/ Inject_control_initialize
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : Inject_classify_peer_envelope(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)
    \/ \E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : Inject_classify_plain_event(arg_raw_item_id, arg_source_name, arg_handling_mode)
    \/ \E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : Inject_runtime_admission_accepted(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)
    \/ \E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : Inject_ingress_stage_drain_snapshot(arg_run_id, arg_contributing_work_ids)

Next ==
    \/ CoreNext
    \/ InjectNext

WitnessNext_trusted_peer_enters_runtime ==
    \/ CoreNext
    \/ WitnessInjectNext_trusted_peer_enters_runtime

WitnessNext_admitted_peer_work_enters_ingress ==
    \/ CoreNext
    \/ WitnessInjectNext_admitted_peer_work_enters_ingress

WitnessNext_peer_ingress_ready_begins_run ==
    \/ CoreNext
    \/ WitnessInjectNext_peer_ingress_ready_begins_run

WitnessNext_control_preempts_peer_delivery ==
    \/ CoreNext
    \/ WitnessInjectNext_control_preempts_peer_delivery


peer_work_enters_runtime_via_canonical_admission == \A input_packet \in observed_inputs : ((input_packet.machine = "runtime_control" /\ input_packet.variant = "SubmitWork") => (/\ input_packet.source_kind = "route" /\ input_packet.source_machine = "peer_comms" /\ input_packet.source_effect = "EnqueueClassifiedEntry" /\ \E effect_packet \in emitted_effects : /\ effect_packet.machine = "peer_comms" /\ effect_packet.variant = "EnqueueClassifiedEntry" /\ effect_packet.effect_id = input_packet.effect_id /\ \E route_packet \in RoutePackets : /\ route_packet.route = input_packet.source_route /\ route_packet.source_machine = "peer_comms" /\ route_packet.effect = "EnqueueClassifiedEntry" /\ route_packet.target_machine = "runtime_control" /\ route_packet.target_input = "SubmitWork" /\ route_packet.effect_id = input_packet.effect_id /\ route_packet.payload = input_packet.payload))
peer_admission_flows_into_ingress == \A input_packet \in observed_inputs : ((input_packet.machine = "runtime_ingress" /\ input_packet.variant = "AdmitQueued") => (/\ input_packet.source_kind = "route" /\ input_packet.source_machine = "runtime_control" /\ input_packet.source_effect = "SubmitAdmittedIngressEffect" /\ \E effect_packet \in emitted_effects : /\ effect_packet.machine = "runtime_control" /\ effect_packet.variant = "SubmitAdmittedIngressEffect" /\ effect_packet.effect_id = input_packet.effect_id /\ \E route_packet \in RoutePackets : /\ route_packet.route = input_packet.source_route /\ route_packet.source_machine = "runtime_control" /\ route_packet.effect = "SubmitAdmittedIngressEffect" /\ route_packet.target_machine = "runtime_ingress" /\ route_packet.target_input = "AdmitQueued" /\ route_packet.effect_id = input_packet.effect_id /\ route_packet.payload = input_packet.payload))
control_preempts_peer_delivery == <<"PreemptWhenReady", "control_plane", "peer_plane">> \in SchedulerRules

RouteObserved_peer_candidate_enters_runtime_admission == \E packet \in RoutePackets : packet.route = "peer_candidate_enters_runtime_admission"
RouteCoverage_peer_candidate_enters_runtime_admission == (RouteObserved_peer_candidate_enters_runtime_admission \/ ~RouteObserved_peer_candidate_enters_runtime_admission)
RouteObserved_admitted_peer_work_enters_ingress == \E packet \in RoutePackets : packet.route = "admitted_peer_work_enters_ingress"
RouteCoverage_admitted_peer_work_enters_ingress == (RouteObserved_admitted_peer_work_enters_ingress \/ ~RouteObserved_admitted_peer_work_enters_ingress)
RouteObserved_peer_ingress_ready_begins_run == \E packet \in RoutePackets : packet.route = "peer_ingress_ready_begins_run"
RouteCoverage_peer_ingress_ready_begins_run == (RouteObserved_peer_ingress_ready_begins_run \/ ~RouteObserved_peer_ingress_ready_begins_run)
SchedulerTriggered_PreemptWhenReady_control_plane_peer_plane == /\ "control_plane" \in PendingActors /\ "peer_plane" \in PendingActors
SchedulerCoverage_PreemptWhenReady_control_plane_peer_plane == (SchedulerTriggered_PreemptWhenReady_control_plane_peer_plane \/ ~SchedulerTriggered_PreemptWhenReady_control_plane_peer_plane)
CoverageInstrumentation == RouteCoverage_peer_candidate_enters_runtime_admission /\ RouteCoverage_admitted_peer_work_enters_ingress /\ RouteCoverage_peer_ingress_ready_begins_run /\ SchedulerCoverage_PreemptWhenReady_control_plane_peer_plane

CiStateConstraint == /\ model_step_count <= 0 /\ Len(pending_inputs) <= 1 /\ Cardinality(observed_inputs) <= 4 /\ Len(pending_routes) <= 1 /\ Cardinality(delivered_routes) <= 1 /\ Cardinality(emitted_effects) <= 1 /\ Cardinality(observed_transitions) <= 0 /\ Cardinality(runtime_ingress_admitted_inputs) <= 1 /\ Len(runtime_ingress_admission_order) <= 1 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 1 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 1 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 1 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 1 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 1 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 1 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 1 /\ Len(runtime_ingress_queue) <= 1 /\ Len(runtime_ingress_steer_queue) <= 1 /\ Len(runtime_ingress_current_run_contributors) <= 1 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 1 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 1 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 1
DeepStateConstraint == /\ model_step_count <= 6 /\ Len(pending_inputs) <= 2 /\ Cardinality(observed_inputs) <= 6 /\ Len(pending_routes) <= 2 /\ Cardinality(delivered_routes) <= 2 /\ Cardinality(emitted_effects) <= 2 /\ Cardinality(observed_transitions) <= 6 /\ Cardinality(runtime_ingress_admitted_inputs) <= 2 /\ Len(runtime_ingress_admission_order) <= 2 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 2 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 2 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 2 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 2 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 2 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 2 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 2 /\ Len(runtime_ingress_queue) <= 2 /\ Len(runtime_ingress_steer_queue) <= 2 /\ Len(runtime_ingress_current_run_contributors) <= 2 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 2 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 2 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 2
WitnessStateConstraint_trusted_peer_enters_runtime == /\ model_step_count <= 5 /\ Len(pending_inputs) <= 4 /\ Cardinality(observed_inputs) <= 8 /\ Len(pending_routes) <= 2 /\ Cardinality(delivered_routes) <= 2 /\ Cardinality(emitted_effects) <= 2 /\ Cardinality(observed_transitions) <= 5 /\ Cardinality(runtime_ingress_admitted_inputs) <= 4 /\ Len(runtime_ingress_admission_order) <= 4 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 4 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 4 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 4 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 4 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 4 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 4 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 4 /\ Len(runtime_ingress_queue) <= 4 /\ Len(runtime_ingress_steer_queue) <= 4 /\ Len(runtime_ingress_current_run_contributors) <= 4 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 4 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 4 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 4
WitnessStateConstraint_admitted_peer_work_enters_ingress == /\ model_step_count <= 6 /\ Len(pending_inputs) <= 5 /\ Cardinality(observed_inputs) <= 10 /\ Len(pending_routes) <= 2 /\ Cardinality(delivered_routes) <= 3 /\ Cardinality(emitted_effects) <= 3 /\ Cardinality(observed_transitions) <= 6 /\ Cardinality(runtime_ingress_admitted_inputs) <= 5 /\ Len(runtime_ingress_admission_order) <= 5 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 5 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 5 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 5 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 5 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 5 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 5 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 5 /\ Len(runtime_ingress_queue) <= 5 /\ Len(runtime_ingress_steer_queue) <= 5 /\ Len(runtime_ingress_current_run_contributors) <= 5 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 5 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 5 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 5
WitnessStateConstraint_peer_ingress_ready_begins_run == /\ model_step_count <= 9 /\ Len(pending_inputs) <= 6 /\ Cardinality(observed_inputs) <= 12 /\ Len(pending_routes) <= 3 /\ Cardinality(delivered_routes) <= 4 /\ Cardinality(emitted_effects) <= 5 /\ Cardinality(observed_transitions) <= 9 /\ Cardinality(runtime_ingress_admitted_inputs) <= 5 /\ Len(runtime_ingress_admission_order) <= 5 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 5 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 5 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 5 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 5 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 5 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 5 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 5 /\ Len(runtime_ingress_queue) <= 5 /\ Len(runtime_ingress_steer_queue) <= 5 /\ Len(runtime_ingress_current_run_contributors) <= 5 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 5 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 5 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 5
WitnessStateConstraint_control_preempts_peer_delivery == /\ model_step_count <= 5 /\ Len(pending_inputs) <= 4 /\ Cardinality(observed_inputs) <= 8 /\ Len(pending_routes) <= 2 /\ Cardinality(delivered_routes) <= 2 /\ Cardinality(emitted_effects) <= 2 /\ Cardinality(observed_transitions) <= 5 /\ Cardinality(runtime_ingress_admitted_inputs) <= 4 /\ Len(runtime_ingress_admission_order) <= 4 /\ Cardinality(DOMAIN runtime_ingress_content_shape) <= 4 /\ Cardinality(DOMAIN runtime_ingress_request_id) <= 4 /\ Cardinality(DOMAIN runtime_ingress_reservation_key) <= 4 /\ Cardinality(DOMAIN runtime_ingress_policy_snapshot) <= 4 /\ Cardinality(DOMAIN runtime_ingress_handling_mode) <= 4 /\ Cardinality(DOMAIN runtime_ingress_lifecycle) <= 4 /\ Cardinality(DOMAIN runtime_ingress_terminal_outcome) <= 4 /\ Len(runtime_ingress_queue) <= 4 /\ Len(runtime_ingress_steer_queue) <= 4 /\ Len(runtime_ingress_current_run_contributors) <= 4 /\ Cardinality(DOMAIN runtime_ingress_last_run) <= 4 /\ Cardinality(DOMAIN runtime_ingress_last_boundary_sequence) <= 4 /\ Cardinality(runtime_ingress_silent_intent_overrides) <= 4

Spec == Init /\ [][Next]_vars
WitnessSpec_trusted_peer_enters_runtime == WitnessInit_trusted_peer_enters_runtime /\ [] [WitnessNext_trusted_peer_enters_runtime]_vars /\ WF_vars(DeliverQueuedRoute) /\ WF_vars(\E arg_require_peer_auth \in BOOLEAN : \E arg_raw_item_id \in RawItemIdValues : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode)) /\ WF_vars(runtime_control_Initialize) /\ WF_vars(runtime_control_AttachFromIdle) /\ WF_vars(runtime_control_DetachToIdle) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRecovering(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledFromRetiredInFlight(arg_run_id)) /\ WF_vars(runtime_control_RecoverRequestedFromIdle) /\ WF_vars(runtime_control_RecoverRequestedFromRunning) /\ WF_vars(runtime_control_RecoverRequestedFromAttached) /\ WF_vars(runtime_control_RecoverySucceeded) /\ WF_vars(runtime_control_RetireRequestedFromIdle) /\ WF_vars(runtime_control_RetireRequestedFromRunning) /\ WF_vars(runtime_control_RetireRequestedFromAttached) /\ WF_vars(runtime_control_ResetRequested) /\ WF_vars(runtime_control_StopRequested) /\ WF_vars(runtime_control_DestroyRequested) /\ WF_vars(runtime_control_ResumeRequested) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id)) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedIdle) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRunning) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRecovering) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRetired) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedAttached) /\ WF_vars(runtime_control_RecycleRequestedFromRetired) /\ WF_vars(runtime_control_RecycleRequestedFromIdle) /\ WF_vars(runtime_control_RecycleRequestedFromAttached) /\ WF_vars(runtime_control_RecycleSucceeded) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromRetired(arg_run_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(runtime_ingress_Retire) /\ WF_vars(runtime_ingress_ResetFromActive) /\ WF_vars(runtime_ingress_ResetFromRetired) /\ WF_vars(runtime_ingress_Destroy) /\ WF_vars(runtime_ingress_RecoverFromActive) /\ WF_vars(runtime_ingress_RecoverFromRetired) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents)) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents)) /\ WF_vars(WitnessInjectNext_trusted_peer_enters_runtime)
WitnessSpec_admitted_peer_work_enters_ingress == WitnessInit_admitted_peer_work_enters_ingress /\ [] [WitnessNext_admitted_peer_work_enters_ingress]_vars /\ WF_vars(DeliverQueuedRoute) /\ WF_vars(\E arg_require_peer_auth \in BOOLEAN : \E arg_raw_item_id \in RawItemIdValues : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode)) /\ WF_vars(runtime_control_Initialize) /\ WF_vars(runtime_control_AttachFromIdle) /\ WF_vars(runtime_control_DetachToIdle) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRecovering(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledFromRetiredInFlight(arg_run_id)) /\ WF_vars(runtime_control_RecoverRequestedFromIdle) /\ WF_vars(runtime_control_RecoverRequestedFromRunning) /\ WF_vars(runtime_control_RecoverRequestedFromAttached) /\ WF_vars(runtime_control_RecoverySucceeded) /\ WF_vars(runtime_control_RetireRequestedFromIdle) /\ WF_vars(runtime_control_RetireRequestedFromRunning) /\ WF_vars(runtime_control_RetireRequestedFromAttached) /\ WF_vars(runtime_control_ResetRequested) /\ WF_vars(runtime_control_StopRequested) /\ WF_vars(runtime_control_DestroyRequested) /\ WF_vars(runtime_control_ResumeRequested) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id)) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedIdle) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRunning) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRecovering) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRetired) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedAttached) /\ WF_vars(runtime_control_RecycleRequestedFromRetired) /\ WF_vars(runtime_control_RecycleRequestedFromIdle) /\ WF_vars(runtime_control_RecycleRequestedFromAttached) /\ WF_vars(runtime_control_RecycleSucceeded) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromRetired(arg_run_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(runtime_ingress_Retire) /\ WF_vars(runtime_ingress_ResetFromActive) /\ WF_vars(runtime_ingress_ResetFromRetired) /\ WF_vars(runtime_ingress_Destroy) /\ WF_vars(runtime_ingress_RecoverFromActive) /\ WF_vars(runtime_ingress_RecoverFromRetired) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents)) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents)) /\ WF_vars(WitnessInjectNext_admitted_peer_work_enters_ingress)
WitnessSpec_peer_ingress_ready_begins_run == WitnessInit_peer_ingress_ready_begins_run /\ [] [WitnessNext_peer_ingress_ready_begins_run]_vars /\ WF_vars(DeliverQueuedRoute) /\ WF_vars(\E arg_require_peer_auth \in BOOLEAN : \E arg_raw_item_id \in RawItemIdValues : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode)) /\ WF_vars(runtime_control_Initialize) /\ WF_vars(runtime_control_AttachFromIdle) /\ WF_vars(runtime_control_DetachToIdle) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRecovering(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledFromRetiredInFlight(arg_run_id)) /\ WF_vars(runtime_control_RecoverRequestedFromIdle) /\ WF_vars(runtime_control_RecoverRequestedFromRunning) /\ WF_vars(runtime_control_RecoverRequestedFromAttached) /\ WF_vars(runtime_control_RecoverySucceeded) /\ WF_vars(runtime_control_RetireRequestedFromIdle) /\ WF_vars(runtime_control_RetireRequestedFromRunning) /\ WF_vars(runtime_control_RetireRequestedFromAttached) /\ WF_vars(runtime_control_ResetRequested) /\ WF_vars(runtime_control_StopRequested) /\ WF_vars(runtime_control_DestroyRequested) /\ WF_vars(runtime_control_ResumeRequested) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id)) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedIdle) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRunning) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRecovering) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRetired) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedAttached) /\ WF_vars(runtime_control_RecycleRequestedFromRetired) /\ WF_vars(runtime_control_RecycleRequestedFromIdle) /\ WF_vars(runtime_control_RecycleRequestedFromAttached) /\ WF_vars(runtime_control_RecycleSucceeded) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromRetired(arg_run_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(runtime_ingress_Retire) /\ WF_vars(runtime_ingress_ResetFromActive) /\ WF_vars(runtime_ingress_ResetFromRetired) /\ WF_vars(runtime_ingress_Destroy) /\ WF_vars(runtime_ingress_RecoverFromActive) /\ WF_vars(runtime_ingress_RecoverFromRetired) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents)) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents)) /\ WF_vars(WitnessInjectNext_peer_ingress_ready_begins_run)
WitnessSpec_control_preempts_peer_delivery == WitnessInit_control_preempts_peer_delivery /\ [] [WitnessNext_control_preempts_peer_delivery]_vars /\ WF_vars(DeliverQueuedRoute) /\ WF_vars(\E arg_require_peer_auth \in BOOLEAN : \E arg_raw_item_id \in RawItemIdValues : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropUntrustedExternal(arg_require_peer_auth, arg_raw_item_id, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DropAckExternal(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_DismissExternalMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleAdded(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleRetired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleUnwired(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffFailed(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueLifecycleKickoffCancelled(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueSilentRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableRequest(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueActionableMessage(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_require_peer_auth \in BOOLEAN : \E arg_sender_name_known \in BOOLEAN : \E arg_sender_name \in StringValues : \E arg_fallback_sender_name \in StringValues : \E arg_kind \in PeerEnvelopeKindValues : \E arg_intent \in StringValues : \E arg_lifecycle_peer_present \in BOOLEAN : \E arg_lifecycle_peer \in StringValues : \E arg_handling_mode_present \in BOOLEAN : \E arg_handling_mode \in HandlingModeValues : \E arg_silent_intent \in BOOLEAN : \E arg_dismiss_message \in BOOLEAN : peer_comms_EnqueueResponse(arg_raw_item_id, arg_require_peer_auth, arg_sender_name_known, arg_sender_name, arg_fallback_sender_name, arg_kind, arg_intent, arg_lifecycle_peer_present, arg_lifecycle_peer, arg_handling_mode_present, arg_handling_mode, arg_silent_intent, arg_dismiss_message)) /\ WF_vars(\E arg_raw_item_id \in RawItemIdValues : \E arg_source_name \in StringValues : \E arg_handling_mode \in HandlingModeValues : peer_comms_EnqueuePlainEvent(arg_raw_item_id, arg_source_name, arg_handling_mode)) /\ WF_vars(runtime_control_Initialize) /\ WF_vars(runtime_control_AttachFromIdle) /\ WF_vars(runtime_control_DetachToIdle) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_BeginRunFromRecovering(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToIdle(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToAttached(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledToRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCompletedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunFailedFromRetiredInFlight(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_control_RunCancelledFromRetiredInFlight(arg_run_id)) /\ WF_vars(runtime_control_RecoverRequestedFromIdle) /\ WF_vars(runtime_control_RecoverRequestedFromRunning) /\ WF_vars(runtime_control_RecoverRequestedFromAttached) /\ WF_vars(runtime_control_RecoverySucceeded) /\ WF_vars(runtime_control_RetireRequestedFromIdle) /\ WF_vars(runtime_control_RetireRequestedFromRunning) /\ WF_vars(runtime_control_RetireRequestedFromAttached) /\ WF_vars(runtime_control_ResetRequested) /\ WF_vars(runtime_control_StopRequested) /\ WF_vars(runtime_control_DestroyRequested) /\ WF_vars(runtime_control_ResumeRequested) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromIdle(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromRunning(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : runtime_control_SubmitWorkFromAttached(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedIdleSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedRunningSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_admission_effect \in AdmissionEffectValues : runtime_control_AdmissionAcceptedAttachedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_admission_effect)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedIdle(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedRunning(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_reason \in StringValues : runtime_control_AdmissionRejectedAttached(arg_work_id, arg_reason)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedIdle(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedRunning(arg_work_id, arg_existing_work_id)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_existing_work_id \in WorkIdValues : runtime_control_AdmissionDeduplicatedAttached(arg_work_id, arg_existing_work_id)) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedIdle) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRunning) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRecovering) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedRetired) /\ WF_vars(runtime_control_ExternalToolDeltaReceivedAttached) /\ WF_vars(runtime_control_RecycleRequestedFromRetired) /\ WF_vars(runtime_control_RecycleRequestedFromIdle) /\ WF_vars(runtime_control_RecycleRequestedFromAttached) /\ WF_vars(runtime_control_RecycleSucceeded) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedQueue(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_handling_mode \in HandlingModeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitQueuedSteer(arg_work_id, arg_content_shape, arg_handling_mode, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_work_id \in WorkIdValues : \E arg_content_shape \in ContentShapeValues : \E arg_request_id \in OptionRequestIdValues : \E arg_reservation_key \in OptionReservationKeyValues : \E arg_policy \in PolicyDecisionValues : runtime_ingress_AdmitConsumedOnAccept(arg_work_id, arg_content_shape, arg_request_id, arg_reservation_key, arg_policy)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromActive(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_contributing_work_ids \in SeqOfWorkIdValues : runtime_ingress_StageDrainSnapshotFromRetired(arg_run_id, arg_contributing_work_ids)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromActive(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : \E arg_boundary_sequence \in 0..2 : runtime_ingress_BoundaryAppliedFromRetired(arg_run_id, arg_boundary_sequence)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCompletedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunFailedFromRetired(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromActive(arg_run_id)) /\ WF_vars(\E arg_run_id \in RunIdValues : runtime_ingress_RunCancelledFromRetired(arg_run_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromActive(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_new_work_id \in WorkIdValues : \E arg_old_work_id \in WorkIdValues : runtime_ingress_SupersedeQueuedInputFromRetired(arg_new_work_id, arg_old_work_id)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromActive(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(\E arg_aggregate_work_id \in WorkIdValues : \E arg_source_work_ids \in SeqOfWorkIdValues : runtime_ingress_CoalesceQueuedInputsFromRetired(arg_aggregate_work_id, arg_source_work_ids)) /\ WF_vars(runtime_ingress_Retire) /\ WF_vars(runtime_ingress_ResetFromActive) /\ WF_vars(runtime_ingress_ResetFromRetired) /\ WF_vars(runtime_ingress_Destroy) /\ WF_vars(runtime_ingress_RecoverFromActive) /\ WF_vars(runtime_ingress_RecoverFromRetired) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromActive(arg_intents)) /\ WF_vars(\E arg_intents \in SetOfStringValues : runtime_ingress_SetSilentIntentOverridesFromRetired(arg_intents)) /\ WF_vars(WitnessInjectNext_control_preempts_peer_delivery)

WitnessRouteObserved_trusted_peer_enters_runtime_peer_candidate_enters_runtime_admission == <> RouteObserved_peer_candidate_enters_runtime_admission
WitnessStateObserved_trusted_peer_enters_runtime_1 == <> (peer_comms_phase = "Ready")
WitnessStateObserved_trusted_peer_enters_runtime_2 == <> (runtime_control_phase = "Idle")
WitnessTransitionObserved_trusted_peer_enters_runtime_runtime_control_Initialize == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_control" /\ packet.transition = "Initialize")
WitnessTransitionObserved_trusted_peer_enters_runtime_peer_comms_EnqueueActionableMessage == <> (\E packet \in observed_transitions : /\ packet.machine = "peer_comms" /\ packet.transition = "EnqueueActionableMessage")
WitnessTransitionOrder_trusted_peer_enters_runtime_1 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "runtime_control" /\ earlier.transition = "Initialize" /\ later.machine = "peer_comms" /\ later.transition = "EnqueueActionableMessage" /\ earlier.step < later.step)
WitnessRouteObserved_admitted_peer_work_enters_ingress_peer_candidate_enters_runtime_admission == <> RouteObserved_peer_candidate_enters_runtime_admission
WitnessRouteObserved_admitted_peer_work_enters_ingress_admitted_peer_work_enters_ingress == <> RouteObserved_admitted_peer_work_enters_ingress
WitnessStateObserved_admitted_peer_work_enters_ingress_1 == <> (runtime_control_phase = "Idle")
WitnessStateObserved_admitted_peer_work_enters_ingress_2 == <> (runtime_ingress_phase = "Active")
WitnessTransitionObserved_admitted_peer_work_enters_ingress_peer_comms_EnqueueActionableRequest == <> (\E packet \in observed_transitions : /\ packet.machine = "peer_comms" /\ packet.transition = "EnqueueActionableRequest")
WitnessTransitionObserved_admitted_peer_work_enters_ingress_runtime_control_AdmissionAcceptedIdleSteer == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_control" /\ packet.transition = "AdmissionAcceptedIdleSteer")
WitnessTransitionObserved_admitted_peer_work_enters_ingress_runtime_ingress_AdmitQueuedSteer == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_ingress" /\ packet.transition = "AdmitQueuedSteer")
WitnessTransitionOrder_admitted_peer_work_enters_ingress_1 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "peer_comms" /\ earlier.transition = "EnqueueActionableRequest" /\ later.machine = "runtime_control" /\ later.transition = "AdmissionAcceptedIdleSteer" /\ earlier.step < later.step)
WitnessTransitionOrder_admitted_peer_work_enters_ingress_2 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "runtime_control" /\ earlier.transition = "AdmissionAcceptedIdleSteer" /\ later.machine = "runtime_ingress" /\ later.transition = "AdmitQueuedSteer" /\ earlier.step < later.step)
WitnessRouteObserved_peer_ingress_ready_begins_run_peer_candidate_enters_runtime_admission == <> RouteObserved_peer_candidate_enters_runtime_admission
WitnessRouteObserved_peer_ingress_ready_begins_run_admitted_peer_work_enters_ingress == <> RouteObserved_admitted_peer_work_enters_ingress
WitnessRouteObserved_peer_ingress_ready_begins_run_peer_ingress_ready_begins_run == <> RouteObserved_peer_ingress_ready_begins_run
WitnessStateObserved_peer_ingress_ready_begins_run_1 == <> (runtime_control_phase = "Running")
WitnessStateObserved_peer_ingress_ready_begins_run_2 == <> (runtime_ingress_phase = "Active")
WitnessTransitionObserved_peer_ingress_ready_begins_run_peer_comms_EnqueueActionableRequest == <> (\E packet \in observed_transitions : /\ packet.machine = "peer_comms" /\ packet.transition = "EnqueueActionableRequest")
WitnessTransitionObserved_peer_ingress_ready_begins_run_runtime_control_AdmissionAcceptedIdleSteer == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_control" /\ packet.transition = "AdmissionAcceptedIdleSteer")
WitnessTransitionObserved_peer_ingress_ready_begins_run_runtime_ingress_AdmitQueuedSteer == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_ingress" /\ packet.transition = "AdmitQueuedSteer")
WitnessTransitionObserved_peer_ingress_ready_begins_run_runtime_ingress_StageDrainSnapshotFromActive == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_ingress" /\ packet.transition = "StageDrainSnapshotFromActive")
WitnessTransitionObserved_peer_ingress_ready_begins_run_runtime_control_BeginRunFromIdle == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_control" /\ packet.transition = "BeginRunFromIdle")
WitnessTransitionOrder_peer_ingress_ready_begins_run_1 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "runtime_ingress" /\ earlier.transition = "AdmitQueuedSteer" /\ later.machine = "runtime_ingress" /\ later.transition = "StageDrainSnapshotFromActive" /\ earlier.step < later.step)
WitnessTransitionOrder_peer_ingress_ready_begins_run_2 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "runtime_ingress" /\ earlier.transition = "StageDrainSnapshotFromActive" /\ later.machine = "runtime_control" /\ later.transition = "BeginRunFromIdle" /\ earlier.step < later.step)
WitnessSchedulerTriggered_control_preempts_peer_delivery_PreemptWhenReady_control_plane_peer_plane == <> SchedulerTriggered_PreemptWhenReady_control_plane_peer_plane
WitnessStateObserved_control_preempts_peer_delivery_1 == <> (runtime_control_phase = "Idle")
WitnessTransitionObserved_control_preempts_peer_delivery_runtime_control_Initialize == <> (\E packet \in observed_transitions : /\ packet.machine = "runtime_control" /\ packet.transition = "Initialize")
WitnessTransitionObserved_control_preempts_peer_delivery_peer_comms_EnqueueActionableMessage == <> (\E packet \in observed_transitions : /\ packet.machine = "peer_comms" /\ packet.transition = "EnqueueActionableMessage")
WitnessTransitionOrder_control_preempts_peer_delivery_1 == <> (\E earlier \in observed_transitions, later \in observed_transitions : /\ earlier.machine = "runtime_control" /\ earlier.transition = "Initialize" /\ later.machine = "peer_comms" /\ later.transition = "EnqueueActionableMessage" /\ earlier.step < later.step)

THEOREM Spec => []peer_work_enters_runtime_via_canonical_admission
THEOREM Spec => []peer_admission_flows_into_ingress
THEOREM Spec => []control_preempts_peer_delivery
THEOREM Spec => []runtime_control_running_implies_active_run
THEOREM Spec => []runtime_control_active_run_only_while_running_or_retired
THEOREM Spec => []runtime_ingress_queue_entries_are_queued
THEOREM Spec => []runtime_ingress_steer_entries_are_queued
THEOREM Spec => []runtime_ingress_pending_inputs_preserve_content_shape
THEOREM Spec => []runtime_ingress_admitted_inputs_preserve_correlation_slots
THEOREM Spec => []runtime_ingress_queue_entries_preserve_handling_mode
THEOREM Spec => []runtime_ingress_steer_entries_preserve_handling_mode
THEOREM Spec => []runtime_ingress_pending_queues_do_not_overlap
THEOREM Spec => []runtime_ingress_terminal_inputs_do_not_appear_in_queue
THEOREM Spec => []runtime_ingress_current_run_matches_contributor_presence
THEOREM Spec => []runtime_ingress_staged_contributors_are_not_queued
THEOREM Spec => []runtime_ingress_applied_pending_consumption_has_last_run

=============================================================================
