---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for MeerkatMachine.

CONSTANTS AgentRuntimeIdValues, BooleanValues, FenceTokenValues, GenerationValues, InboundPeerRequestStateValues, InputIdValues, McpServerIdValues, McpServerStateValues, NatValues, OutboundPeerRequestStateValues, PeerCorrelationIdValues, PeerTerminalDispositionValues, RealtimeBindingStateValues, RunIdValues, SessionIdValues, SessionLlmCapabilitySurfaceStatusValues, SessionLlmCapabilitySurfaceValues, SessionLlmIdentityValues, SessionToolVisibilityDeltaValues, SessionToolVisibilityStateValues, SetOfStringValues, StringValues, ToolFilterValues, ToolVisibilityWitnessValues, WorkIdValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

OptionAgentRuntimeIdValues == {None} \cup {Some(x) : x \in AgentRuntimeIdValues}
OptionFenceTokenValues == {None} \cup {Some(x) : x \in FenceTokenValues}
OptionRunIdValues == {None} \cup {Some(x) : x \in RunIdValues}
OptionSessionIdValues == {None} \cup {Some(x) : x \in SessionIdValues}
OptionSessionLlmCapabilitySurfaceValues == {None} \cup {Some(x) : x \in SessionLlmCapabilitySurfaceValues}
OptionStringValues == {None} \cup {Some(x) : x \in StringValues}
OptionU64Values == {None} \cup {Some(x) : x \in NatValues}
MapMcpServerIdMcpServerStateValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in McpServerIdValues, v \in McpServerStateValues }
MapPeerCorrelationIdInboundPeerRequestStateValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in PeerCorrelationIdValues, v \in InboundPeerRequestStateValues }
MapPeerCorrelationIdOutboundPeerRequestStateValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in PeerCorrelationIdValues, v \in OutboundPeerRequestStateValues }
MapStringToolVisibilityWitnessValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in ToolVisibilityWitnessValues }

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
MapRemove(map, key) == [x \in DOMAIN map \ {key} |-> map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN SeqRemove(Tail(seq), value) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))

VARIABLES phase, model_step_count, session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms

vars == << phase, model_step_count, session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>

Init ==
    /\ phase = "Initializing"
    /\ model_step_count = 0
    /\ session_id = None
    /\ active_runtime_id = None
    /\ active_fence_token = None
    /\ current_run_id = None
    /\ pre_run_phase = None
    /\ silent_intent_overrides = {}
    /\ realtime_intent_present = FALSE
    /\ realtime_binding_state = "Unbound"
    /\ realtime_binding_authority_epoch = None
    /\ realtime_reattach_required = FALSE
    /\ realtime_next_authority_epoch = 1
    /\ live_topology_phase = "Idle"
    /\ mcp_server_states = [x \in {} |-> None]
    /\ pending_peer_requests = [x \in {} |-> None]
    /\ inbound_peer_requests = [x \in {} |-> None]
    /\ last_session_context_updated_at_ms = 0

TerminalStutter ==
    /\ phase = "Destroyed"
    /\ UNCHANGED vars

Initialize ==
    /\ phase = "Initializing"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RegisterSessionIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RegisterSessionAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RegisterSessionRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RegisterSessionRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RegisterSessionStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


UnregisterSessionIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ (session_id = Some(arg_session_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = None
    /\ active_runtime_id' = None
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


UnregisterSessionAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ (session_id = Some(arg_session_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = None
    /\ active_runtime_id' = None
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


UnregisterSessionRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ (session_id = Some(arg_session_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = None
    /\ active_runtime_id' = None
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


UnregisterSessionRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ (session_id = Some(arg_session_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = None
    /\ active_runtime_id' = None
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


UnregisterSessionStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ (session_id = Some(arg_session_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = None
    /\ active_runtime_id' = None
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReconfigureSessionLlmIdentityAttached(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (active_runtime_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReconfigureSessionLlmIdentityRunning(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (active_runtime_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StagePersistentFilterIdle(filter, witnesses) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StagePersistentFilterAttached(filter, witnesses) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StagePersistentFilterRunning(filter, witnesses) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StagePersistentFilterRetired(filter, witnesses) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StagePersistentFilterStopped(filter, witnesses) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequestDeferredToolsIdle(names, witnesses) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequestDeferredToolsAttached(names, witnesses) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequestDeferredToolsRunning(names, witnesses) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequestDeferredToolsRetired(names, witnesses) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequestDeferredToolsStopped(names, witnesses) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsInitializing(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Initializing"
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsIdle(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Idle"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsAttached(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsRunning(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsRetired(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareBindingsStopped(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetPeerIngressContextIdle(keep_alive) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetPeerIngressContextAttached(keep_alive) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetPeerIngressContextRunning(keep_alive) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetPeerIngressContextRetired(keep_alive) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetPeerIngressContextStopped(keep_alive) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


NotifyDrainExitedIdle(reason) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


NotifyDrainExitedAttached(reason) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


NotifyDrainExitedRunning(reason) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


NotifyDrainExitedRetired(reason) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


NotifyDrainExitedStopped(reason) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


InterruptCurrentRunAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


InterruptCurrentRun ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CancelAfterBoundaryAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CancelAfterBoundary ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BoundaryAppliedPublish(revision) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishCommittedVisibleSetIdle(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishCommittedVisibleSetAttached(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishCommittedVisibleSetRunning(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishCommittedVisibleSetRetired(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishCommittedVisibleSetStopped(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RetireRequestedFromIdle ==
    /\ phase = "Idle" \/ phase = "Attached" \/ phase = "Running"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


Reset ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Attached" \/ phase = "Retired"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StopRuntimeExecutorUnbound ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Retired"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StopRuntimeExecutorAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StopRuntimeExecutorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


Destroy ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Attached" \/ phase = "Running" \/ phase = "Retired" \/ phase = "Stopped"
    /\ (active_runtime_id # None)
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureSessionWithExecutorIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureSessionWithExecutorAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureSessionWithExecutorRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureSessionWithExecutorRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureSessionWithExecutorStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetSilentIntentsIdle(arg_session_id, intents) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetSilentIntentsAttached(arg_session_id, intents) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetSilentIntentsRunning(arg_session_id, intents) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetSilentIntentsRetired(arg_session_id, intents) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SetSilentIntentsStopped(arg_session_id, intents) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


WaitIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


WaitAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


WaitRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


WaitRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


WaitStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAllIdle ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAllAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAllRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAllRetired ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortAllStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureDrainRunningAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


EnsureDrainRunningRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


IngestIdle(runtime_id, work_id, origin) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


IngestAttached(runtime_id, work_id, origin) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


IngestRunning(runtime_id, work_id, origin) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishEventIdle(kind) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishEventAttached(kind) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishEventRunning(kind) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishEventRetired(kind) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishEventStopped(kind) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionIdleQueued(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionIdleImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionAttachedImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("attached")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionAttachedQueued(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionRunningQueuedPassive(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ (wake_if_idle = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionRunningQueuedWakeIfIdle(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ (wake_if_idle = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionRunningInterruptYielding(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithCompletionRunningImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithoutWakeIdle(input_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithoutWakeAttached(input_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AcceptWithoutWakeRunning(input_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ClassifyExternalEnvelopeAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ClassifyExternalEnvelopeRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ClassifyPlainEventAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ClassifyPlainEventRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareIdle(arg_session_id, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("idle")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PrepareAttached(arg_session_id, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("attached")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DrainQueuedRunRetired(run_id) ==
    /\ phase = "Retired"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("retired")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StartConversationRunAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StartImmediateAppendAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StartImmediateContextAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CommitRunningToIdle(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("idle"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CommitRunningToAttached(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("attached"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CommitRunningToRetired(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("retired"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailRunningToIdle(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("idle"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailRunningToAttached(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("attached"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailRunningToRetired(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("retired"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageAddAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageAddRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageRemoveAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageRemoveRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageReloadAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


StageReloadRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplySurfaceBoundaryAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplySurfaceBoundaryRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PendingSucceededAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PendingSucceededRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PendingFailedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PendingFailedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CallStartedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CallStartedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CallFinishedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CallFinishedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FinalizeRemovalCleanAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FinalizeRemovalCleanRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FinalizeRemovalForcedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FinalizeRemovalForcedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SnapshotAlignedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


SnapshotAlignedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ShutdownSurfaceAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ShutdownSurfaceRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RecycleFromIdleOrRetired ==
    /\ phase = "Idle" \/ phase = "Retired"
    /\ (active_runtime_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ UNCHANGED << session_id, active_runtime_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RecycleFromAttached ==
    /\ phase = "Attached"
    /\ (active_runtime_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ UNCHANGED << session_id, active_runtime_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ProjectRealtimeIntentIdle(present) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_intent_present' = present
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ProjectRealtimeIntentAttached(present) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_intent_present' = present
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ProjectRealtimeIntentRunning(present) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_intent_present' = present
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ProjectRealtimeIntentRetired(present) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_intent_present' = present
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ProjectRealtimeIntentStopped(present) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_intent_present' = present
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginRealtimeBindingIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "BindingNotReady"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginRealtimeBindingAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "BindingNotReady"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginRealtimeBindingRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "BindingNotReady"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginRealtimeBindingRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "BindingNotReady"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginRealtimeBindingStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "BindingNotReady"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReplaceRealtimeBindingIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "ReplacementPending"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReplaceRealtimeBindingAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "ReplacementPending"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReplaceRealtimeBindingRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "ReplacementPending"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReplaceRealtimeBindingRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "ReplacementPending"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ReplaceRealtimeBindingStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "ReplacementPending"
    /\ realtime_binding_authority_epoch' = Some(realtime_next_authority_epoch)
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DetachRealtimeBindingIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DetachRealtimeBindingAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DetachRealtimeBindingRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DetachRealtimeBindingRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


DetachRealtimeBindingStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequireRealtimeReattachIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequireRealtimeReattachAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequireRealtimeReattachRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequireRealtimeReattachRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


RequireRealtimeReattachStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishRealtimeSignalIdle(authority_epoch, next_binding_state) ==
    /\ phase = "Idle"
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ ((next_binding_state = "BindingNotReady") \/ (next_binding_state = "BindingReady") \/ (next_binding_state = "ReplacementPending"))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = next_binding_state
    /\ realtime_reattach_required' = FALSE
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_authority_epoch, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishRealtimeSignalAttached(authority_epoch, next_binding_state) ==
    /\ phase = "Attached"
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ ((next_binding_state = "BindingNotReady") \/ (next_binding_state = "BindingReady") \/ (next_binding_state = "ReplacementPending"))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = next_binding_state
    /\ realtime_reattach_required' = FALSE
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_authority_epoch, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishRealtimeSignalRunning(authority_epoch, next_binding_state) ==
    /\ phase = "Running"
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ ((next_binding_state = "BindingNotReady") \/ (next_binding_state = "BindingReady") \/ (next_binding_state = "ReplacementPending"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = next_binding_state
    /\ realtime_reattach_required' = FALSE
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_authority_epoch, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishRealtimeSignalRetired(authority_epoch, next_binding_state) ==
    /\ phase = "Retired"
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ ((next_binding_state = "BindingNotReady") \/ (next_binding_state = "BindingReady") \/ (next_binding_state = "ReplacementPending"))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = next_binding_state
    /\ realtime_reattach_required' = FALSE
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_authority_epoch, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PublishRealtimeSignalStopped(authority_epoch, next_binding_state) ==
    /\ phase = "Stopped"
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ ((next_binding_state = "BindingNotReady") \/ (next_binding_state = "BindingReady") \/ (next_binding_state = "ReplacementPending"))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = next_binding_state
    /\ realtime_reattach_required' = FALSE
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_authority_epoch, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectPendingIdle(server_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectPendingAttached(server_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectPendingRunning(server_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectPendingRetired(server_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectPendingStopped(server_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectedIdle(server_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Connected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectedAttached(server_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Connected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectedRunning(server_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Connected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectedRetired(server_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Connected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerConnectedStopped(server_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Connected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerFailedIdle(server_id, error) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Failed")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerFailedAttached(server_id, error) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Failed")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerFailedRunning(server_id, error) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Failed")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerFailedRetired(server_id, error) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Failed")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerFailedStopped(server_id, error) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Failed")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerDisconnectedIdle(server_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Disconnected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerDisconnectedAttached(server_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Disconnected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerDisconnectedRunning(server_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Disconnected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerDisconnectedRetired(server_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Disconnected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerDisconnectedStopped(server_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "Disconnected")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerReloadIdle(server_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerReloadAttached(server_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerReloadRunning(server_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerReloadRetired(server_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


McpServerReloadStopped(server_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ mcp_server_states' = MapSet(mcp_server_states, server_id, "PendingConnect")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestSentIdle(corr_id, to) ==
    /\ phase = "Idle"
    /\ ~((corr_id \in DOMAIN pending_peer_requests))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "Sent")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestSentAttached(corr_id, to) ==
    /\ phase = "Attached"
    /\ ~((corr_id \in DOMAIN pending_peer_requests))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "Sent")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestSentRunning(corr_id, to) ==
    /\ phase = "Running"
    /\ ~((corr_id \in DOMAIN pending_peer_requests))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "Sent")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestSentRetired(corr_id, to) ==
    /\ phase = "Retired"
    /\ ~((corr_id \in DOMAIN pending_peer_requests))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "Sent")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestSentStopped(corr_id, to) ==
    /\ phase = "Stopped"
    /\ ~((corr_id \in DOMAIN pending_peer_requests))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "Sent")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseProgressArrivedIdle(corr_id) ==
    /\ phase = "Idle"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "AcceptedProgress")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseProgressArrivedAttached(corr_id) ==
    /\ phase = "Attached"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "AcceptedProgress")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseProgressArrivedRunning(corr_id) ==
    /\ phase = "Running"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "AcceptedProgress")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseProgressArrivedRetired(corr_id) ==
    /\ phase = "Retired"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "AcceptedProgress")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseProgressArrivedStopped(corr_id) ==
    /\ phase = "Stopped"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapSet(pending_peer_requests, corr_id, "AcceptedProgress")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedCompletedIdle(corr_id, disposition) ==
    /\ phase = "Idle"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Completed")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedCompletedAttached(corr_id, disposition) ==
    /\ phase = "Attached"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Completed")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedCompletedRunning(corr_id, disposition) ==
    /\ phase = "Running"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Completed")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedCompletedRetired(corr_id, disposition) ==
    /\ phase = "Retired"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Completed")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedCompletedStopped(corr_id, disposition) ==
    /\ phase = "Stopped"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Completed")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedFailedIdle(corr_id, disposition) ==
    /\ phase = "Idle"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Failed")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedFailedAttached(corr_id, disposition) ==
    /\ phase = "Attached"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Failed")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedFailedRunning(corr_id, disposition) ==
    /\ phase = "Running"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Failed")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedFailedRetired(corr_id, disposition) ==
    /\ phase = "Retired"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Failed")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerResponseTerminalArrivedFailedStopped(corr_id, disposition) ==
    /\ phase = "Stopped"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ (disposition = "Failed")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestTimedOutIdle(corr_id) ==
    /\ phase = "Idle"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestTimedOutAttached(corr_id) ==
    /\ phase = "Attached"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestTimedOutRunning(corr_id) ==
    /\ phase = "Running"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestTimedOutRetired(corr_id) ==
    /\ phase = "Retired"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestTimedOutStopped(corr_id) ==
    /\ phase = "Stopped"
    /\ (corr_id \in DOMAIN pending_peer_requests)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_peer_requests' = MapRemove(pending_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, inbound_peer_requests, last_session_context_updated_at_ms >>


PeerRequestReceivedIdle(corr_id) ==
    /\ phase = "Idle"
    /\ ~((corr_id \in DOMAIN inbound_peer_requests))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapSet(inbound_peer_requests, corr_id, "Received")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerRequestReceivedAttached(corr_id) ==
    /\ phase = "Attached"
    /\ ~((corr_id \in DOMAIN inbound_peer_requests))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapSet(inbound_peer_requests, corr_id, "Received")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerRequestReceivedRunning(corr_id) ==
    /\ phase = "Running"
    /\ ~((corr_id \in DOMAIN inbound_peer_requests))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapSet(inbound_peer_requests, corr_id, "Received")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerRequestReceivedRetired(corr_id) ==
    /\ phase = "Retired"
    /\ ~((corr_id \in DOMAIN inbound_peer_requests))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapSet(inbound_peer_requests, corr_id, "Received")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerRequestReceivedStopped(corr_id) ==
    /\ phase = "Stopped"
    /\ ~((corr_id \in DOMAIN inbound_peer_requests))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapSet(inbound_peer_requests, corr_id, "Received")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerResponseRepliedIdle(corr_id) ==
    /\ phase = "Idle"
    /\ (corr_id \in DOMAIN inbound_peer_requests)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapRemove(inbound_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerResponseRepliedAttached(corr_id) ==
    /\ phase = "Attached"
    /\ (corr_id \in DOMAIN inbound_peer_requests)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapRemove(inbound_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerResponseRepliedRunning(corr_id) ==
    /\ phase = "Running"
    /\ (corr_id \in DOMAIN inbound_peer_requests)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapRemove(inbound_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerResponseRepliedRetired(corr_id) ==
    /\ phase = "Retired"
    /\ (corr_id \in DOMAIN inbound_peer_requests)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapRemove(inbound_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


PeerResponseRepliedStopped(corr_id) ==
    /\ phase = "Stopped"
    /\ (corr_id \in DOMAIN inbound_peer_requests)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ inbound_peer_requests' = MapRemove(inbound_peer_requests, corr_id)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, last_session_context_updated_at_ms >>


AdvanceSessionContextIdle(updated_at_ms) ==
    /\ phase = "Idle"
    /\ (updated_at_ms > last_session_context_updated_at_ms)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ last_session_context_updated_at_ms' = updated_at_ms
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests >>


AdvanceSessionContextAttached(updated_at_ms) ==
    /\ phase = "Attached"
    /\ (updated_at_ms > last_session_context_updated_at_ms)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ last_session_context_updated_at_ms' = updated_at_ms
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests >>


AdvanceSessionContextRunning(updated_at_ms) ==
    /\ phase = "Running"
    /\ (updated_at_ms > last_session_context_updated_at_ms)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ last_session_context_updated_at_ms' = updated_at_ms
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests >>


AdvanceSessionContextRetired(updated_at_ms) ==
    /\ phase = "Retired"
    /\ (updated_at_ms > last_session_context_updated_at_ms)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ last_session_context_updated_at_ms' = updated_at_ms
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests >>


AdvanceSessionContextStopped(updated_at_ms) ==
    /\ phase = "Stopped"
    /\ (updated_at_ms > last_session_context_updated_at_ms)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ last_session_context_updated_at_ms' = updated_at_ms
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, live_topology_phase, mcp_server_states, pending_peer_requests, inbound_peer_requests >>


BeginLiveTopologyReconfigureIdle(authority_epoch) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Reconfiguring"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginLiveTopologyReconfigureAttached(authority_epoch) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Reconfiguring"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginLiveTopologyReconfigureRunning(authority_epoch) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Reconfiguring"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginLiveTopologyReconfigureRetired(authority_epoch) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Reconfiguring"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


BeginLiveTopologyReconfigureStopped(authority_epoch) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (realtime_binding_authority_epoch = Some(authority_epoch))
    /\ (live_topology_phase = "Idle")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Reconfiguring"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


MarkLiveTopologyDetachedIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ (current_run_id = None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Detached"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


MarkLiveTopologyDetachedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ (current_run_id = None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Detached"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


MarkLiveTopologyDetachedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ (current_run_id = None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Detached"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


MarkLiveTopologyDetachedRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ (current_run_id = None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Detached"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


MarkLiveTopologyDetachedStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ (current_run_id = None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = FALSE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Detached"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyIdentityIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "Detached")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostIdentityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyIdentityAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "Detached")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostIdentityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyIdentityRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "Detached")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostIdentityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyIdentityRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "Detached")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostIdentityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyIdentityStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "Detached")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostIdentityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyVisibilityIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostIdentityApplied")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostVisibilityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyVisibilityAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostIdentityApplied")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostVisibilityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyVisibilityRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostIdentityApplied")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostVisibilityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyVisibilityRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostIdentityApplied")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostVisibilityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


ApplyLiveTopologyVisibilityStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostIdentityApplied")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "HostVisibilityApplied"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CompleteLiveTopologyIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostVisibilityApplied")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CompleteLiveTopologyAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostVisibilityApplied")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CompleteLiveTopologyRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostVisibilityApplied")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CompleteLiveTopologyRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostVisibilityApplied")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


CompleteLiveTopologyStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "HostVisibilityApplied")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortLiveTopologyBeforeDetachIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortLiveTopologyBeforeDetachAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortLiveTopologyBeforeDetachRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortLiveTopologyBeforeDetachRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


AbortLiveTopologyBeforeDetachStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (live_topology_phase = "Reconfiguring")
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, realtime_binding_state, realtime_binding_authority_epoch, realtime_reattach_required, realtime_next_authority_epoch, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailLiveTopologyAfterDetachIdle ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ ((live_topology_phase = "Detached") \/ (live_topology_phase = "HostIdentityApplied") \/ (live_topology_phase = "HostVisibilityApplied"))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailLiveTopologyAfterDetachAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ ((live_topology_phase = "Detached") \/ (live_topology_phase = "HostIdentityApplied") \/ (live_topology_phase = "HostVisibilityApplied"))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailLiveTopologyAfterDetachRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ ((live_topology_phase = "Detached") \/ (live_topology_phase = "HostIdentityApplied") \/ (live_topology_phase = "HostVisibilityApplied"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailLiveTopologyAfterDetachRetired ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ ((live_topology_phase = "Detached") \/ (live_topology_phase = "HostIdentityApplied") \/ (live_topology_phase = "HostVisibilityApplied"))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


FailLiveTopologyAfterDetachStopped ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ ((live_topology_phase = "Detached") \/ (live_topology_phase = "HostIdentityApplied") \/ (live_topology_phase = "HostVisibilityApplied"))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ realtime_binding_state' = "Unbound"
    /\ realtime_binding_authority_epoch' = None
    /\ realtime_reattach_required' = TRUE
    /\ realtime_next_authority_epoch' = (realtime_next_authority_epoch + 1)
    /\ live_topology_phase' = "Idle"
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, realtime_intent_present, mcp_server_states, pending_peer_requests, inbound_peer_requests, last_session_context_updated_at_ms >>


Next ==
    \/ Initialize
    \/ \E arg_session_id \in SessionIdValues : RegisterSessionIdle(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : RegisterSessionAttached(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : RegisterSessionRunning(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : RegisterSessionRetired(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : RegisterSessionStopped(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : UnregisterSessionIdle(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : UnregisterSessionAttached(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : UnregisterSessionRunning(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : UnregisterSessionRetired(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : UnregisterSessionStopped(arg_session_id)
    \/ \E previous_identity \in SessionLlmIdentityValues : \E previous_visibility_state \in SessionToolVisibilityStateValues : \E previous_capability_surface \in OptionSessionLlmCapabilitySurfaceValues : \E previous_capability_surface_status \in SessionLlmCapabilitySurfaceStatusValues : \E target_identity \in SessionLlmIdentityValues : \E target_capability_surface \in SessionLlmCapabilitySurfaceValues : \E next_visibility_state \in SessionToolVisibilityStateValues : \E next_capability_base_filter \in ToolFilterValues : \E next_active_visibility_revision \in 0..2 : \E tool_visibility_delta \in SessionToolVisibilityDeltaValues : ReconfigureSessionLlmIdentityAttached(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta)
    \/ \E previous_identity \in SessionLlmIdentityValues : \E previous_visibility_state \in SessionToolVisibilityStateValues : \E previous_capability_surface \in OptionSessionLlmCapabilitySurfaceValues : \E previous_capability_surface_status \in SessionLlmCapabilitySurfaceStatusValues : \E target_identity \in SessionLlmIdentityValues : \E target_capability_surface \in SessionLlmCapabilitySurfaceValues : \E next_visibility_state \in SessionToolVisibilityStateValues : \E next_capability_base_filter \in ToolFilterValues : \E next_active_visibility_revision \in 0..2 : \E tool_visibility_delta \in SessionToolVisibilityDeltaValues : ReconfigureSessionLlmIdentityRunning(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta)
    \/ \E filter \in ToolFilterValues : \E witnesses \in MapStringToolVisibilityWitnessValues : StagePersistentFilterIdle(filter, witnesses)
    \/ \E filter \in ToolFilterValues : \E witnesses \in MapStringToolVisibilityWitnessValues : StagePersistentFilterAttached(filter, witnesses)
    \/ \E filter \in ToolFilterValues : \E witnesses \in MapStringToolVisibilityWitnessValues : StagePersistentFilterRunning(filter, witnesses)
    \/ \E filter \in ToolFilterValues : \E witnesses \in MapStringToolVisibilityWitnessValues : StagePersistentFilterRetired(filter, witnesses)
    \/ \E filter \in ToolFilterValues : \E witnesses \in MapStringToolVisibilityWitnessValues : StagePersistentFilterStopped(filter, witnesses)
    \/ \E names \in SetOfStringValues : \E witnesses \in MapStringToolVisibilityWitnessValues : RequestDeferredToolsIdle(names, witnesses)
    \/ \E names \in SetOfStringValues : \E witnesses \in MapStringToolVisibilityWitnessValues : RequestDeferredToolsAttached(names, witnesses)
    \/ \E names \in SetOfStringValues : \E witnesses \in MapStringToolVisibilityWitnessValues : RequestDeferredToolsRunning(names, witnesses)
    \/ \E names \in SetOfStringValues : \E witnesses \in MapStringToolVisibilityWitnessValues : RequestDeferredToolsRetired(names, witnesses)
    \/ \E names \in SetOfStringValues : \E witnesses \in MapStringToolVisibilityWitnessValues : RequestDeferredToolsStopped(names, witnesses)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsInitializing(agent_runtime_id, fence_token, generation)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsIdle(agent_runtime_id, fence_token, generation)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsAttached(agent_runtime_id, fence_token, generation)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsRunning(agent_runtime_id, fence_token, generation)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsRetired(agent_runtime_id, fence_token, generation)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : PrepareBindingsStopped(agent_runtime_id, fence_token, generation)
    \/ \E keep_alive \in BOOLEAN : SetPeerIngressContextIdle(keep_alive)
    \/ \E keep_alive \in BOOLEAN : SetPeerIngressContextAttached(keep_alive)
    \/ \E keep_alive \in BOOLEAN : SetPeerIngressContextRunning(keep_alive)
    \/ \E keep_alive \in BOOLEAN : SetPeerIngressContextRetired(keep_alive)
    \/ \E keep_alive \in BOOLEAN : SetPeerIngressContextStopped(keep_alive)
    \/ \E reason \in StringValues : NotifyDrainExitedIdle(reason)
    \/ \E reason \in StringValues : NotifyDrainExitedAttached(reason)
    \/ \E reason \in StringValues : NotifyDrainExitedRunning(reason)
    \/ \E reason \in StringValues : NotifyDrainExitedRetired(reason)
    \/ \E reason \in StringValues : NotifyDrainExitedStopped(reason)
    \/ InterruptCurrentRunAttached
    \/ InterruptCurrentRun
    \/ CancelAfterBoundaryAttached
    \/ CancelAfterBoundary
    \/ \E revision \in 0..2 : BoundaryAppliedPublish(revision)
    \/ \E active_filter \in ToolFilterValues : \E staged_filter \in ToolFilterValues : \E active_requested_deferred_names \in SetOfStringValues : \E staged_requested_deferred_names \in SetOfStringValues : \E active_visibility_revision \in 0..2 : \E staged_visibility_revision \in 0..2 : PublishCommittedVisibleSetIdle(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision)
    \/ \E active_filter \in ToolFilterValues : \E staged_filter \in ToolFilterValues : \E active_requested_deferred_names \in SetOfStringValues : \E staged_requested_deferred_names \in SetOfStringValues : \E active_visibility_revision \in 0..2 : \E staged_visibility_revision \in 0..2 : PublishCommittedVisibleSetAttached(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision)
    \/ \E active_filter \in ToolFilterValues : \E staged_filter \in ToolFilterValues : \E active_requested_deferred_names \in SetOfStringValues : \E staged_requested_deferred_names \in SetOfStringValues : \E active_visibility_revision \in 0..2 : \E staged_visibility_revision \in 0..2 : PublishCommittedVisibleSetRunning(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision)
    \/ \E active_filter \in ToolFilterValues : \E staged_filter \in ToolFilterValues : \E active_requested_deferred_names \in SetOfStringValues : \E staged_requested_deferred_names \in SetOfStringValues : \E active_visibility_revision \in 0..2 : \E staged_visibility_revision \in 0..2 : PublishCommittedVisibleSetRetired(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision)
    \/ \E active_filter \in ToolFilterValues : \E staged_filter \in ToolFilterValues : \E active_requested_deferred_names \in SetOfStringValues : \E staged_requested_deferred_names \in SetOfStringValues : \E active_visibility_revision \in 0..2 : \E staged_visibility_revision \in 0..2 : PublishCommittedVisibleSetStopped(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision)
    \/ RetireRequestedFromIdle
    \/ Reset
    \/ StopRuntimeExecutorUnbound
    \/ StopRuntimeExecutorAttached
    \/ StopRuntimeExecutorRunning
    \/ Destroy
    \/ \E arg_session_id \in SessionIdValues : EnsureSessionWithExecutorIdle(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : EnsureSessionWithExecutorAttached(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : EnsureSessionWithExecutorRunning(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : EnsureSessionWithExecutorRetired(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : EnsureSessionWithExecutorStopped(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : \E intents \in SetOfStringValues : SetSilentIntentsIdle(arg_session_id, intents)
    \/ \E arg_session_id \in SessionIdValues : \E intents \in SetOfStringValues : SetSilentIntentsAttached(arg_session_id, intents)
    \/ \E arg_session_id \in SessionIdValues : \E intents \in SetOfStringValues : SetSilentIntentsRunning(arg_session_id, intents)
    \/ \E arg_session_id \in SessionIdValues : \E intents \in SetOfStringValues : SetSilentIntentsRetired(arg_session_id, intents)
    \/ \E arg_session_id \in SessionIdValues : \E intents \in SetOfStringValues : SetSilentIntentsStopped(arg_session_id, intents)
    \/ \E arg_session_id \in SessionIdValues : AbortIdle(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : AbortAttached(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : AbortRunning(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : AbortRetired(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : AbortStopped(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : WaitIdle(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : WaitAttached(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : WaitRunning(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : WaitRetired(arg_session_id)
    \/ \E arg_session_id \in SessionIdValues : WaitStopped(arg_session_id)
    \/ AbortAllIdle
    \/ AbortAllAttached
    \/ AbortAllRunning
    \/ AbortAllRetired
    \/ AbortAllStopped
    \/ EnsureDrainRunningAttached
    \/ EnsureDrainRunningRunning
    \/ \E runtime_id \in AgentRuntimeIdValues : \E work_id \in WorkIdValues : \E origin \in StringValues : IngestIdle(runtime_id, work_id, origin)
    \/ \E runtime_id \in AgentRuntimeIdValues : \E work_id \in WorkIdValues : \E origin \in StringValues : IngestAttached(runtime_id, work_id, origin)
    \/ \E runtime_id \in AgentRuntimeIdValues : \E work_id \in WorkIdValues : \E origin \in StringValues : IngestRunning(runtime_id, work_id, origin)
    \/ \E kind \in StringValues : PublishEventIdle(kind)
    \/ \E kind \in StringValues : PublishEventAttached(kind)
    \/ \E kind \in StringValues : PublishEventRunning(kind)
    \/ \E kind \in StringValues : PublishEventRetired(kind)
    \/ \E kind \in StringValues : PublishEventStopped(kind)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionIdleQueued(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionIdleImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionAttachedImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionAttachedQueued(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningQueuedPassive(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningQueuedWakeIfIdle(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningInterruptYielding(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E wake_if_idle \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningImmediate(input_id, request_immediate_processing, interrupt_yielding, wake_if_idle, run_id)
    \/ \E input_id \in InputIdValues : AcceptWithoutWakeIdle(input_id)
    \/ \E input_id \in InputIdValues : AcceptWithoutWakeAttached(input_id)
    \/ \E input_id \in InputIdValues : AcceptWithoutWakeRunning(input_id)
    \/ ClassifyExternalEnvelopeAttached
    \/ ClassifyExternalEnvelopeRunning
    \/ ClassifyPlainEventAttached
    \/ ClassifyPlainEventRunning
    \/ \E arg_session_id \in SessionIdValues : \E run_id \in RunIdValues : PrepareIdle(arg_session_id, run_id)
    \/ \E arg_session_id \in SessionIdValues : \E run_id \in RunIdValues : PrepareAttached(arg_session_id, run_id)
    \/ \E run_id \in RunIdValues : DrainQueuedRunRetired(run_id)
    \/ StartConversationRunAttached
    \/ StartImmediateAppendAttached
    \/ StartImmediateContextAttached
    \/ \E input_id \in InputIdValues : \E run_id \in RunIdValues : CommitRunningToIdle(input_id, run_id)
    \/ \E input_id \in InputIdValues : \E run_id \in RunIdValues : CommitRunningToAttached(input_id, run_id)
    \/ \E input_id \in InputIdValues : \E run_id \in RunIdValues : CommitRunningToRetired(input_id, run_id)
    \/ \E run_id \in RunIdValues : FailRunningToIdle(run_id)
    \/ \E run_id \in RunIdValues : FailRunningToAttached(run_id)
    \/ \E run_id \in RunIdValues : FailRunningToRetired(run_id)
    \/ StageAddAttached
    \/ StageAddRunning
    \/ StageRemoveAttached
    \/ StageRemoveRunning
    \/ StageReloadAttached
    \/ StageReloadRunning
    \/ ApplySurfaceBoundaryAttached
    \/ ApplySurfaceBoundaryRunning
    \/ PendingSucceededAttached
    \/ PendingSucceededRunning
    \/ PendingFailedAttached
    \/ PendingFailedRunning
    \/ CallStartedAttached
    \/ CallStartedRunning
    \/ CallFinishedAttached
    \/ CallFinishedRunning
    \/ FinalizeRemovalCleanAttached
    \/ FinalizeRemovalCleanRunning
    \/ FinalizeRemovalForcedAttached
    \/ FinalizeRemovalForcedRunning
    \/ SnapshotAlignedAttached
    \/ SnapshotAlignedRunning
    \/ ShutdownSurfaceAttached
    \/ ShutdownSurfaceRunning
    \/ RecycleFromIdleOrRetired
    \/ RecycleFromAttached
    \/ \E present \in BOOLEAN : ProjectRealtimeIntentIdle(present)
    \/ \E present \in BOOLEAN : ProjectRealtimeIntentAttached(present)
    \/ \E present \in BOOLEAN : ProjectRealtimeIntentRunning(present)
    \/ \E present \in BOOLEAN : ProjectRealtimeIntentRetired(present)
    \/ \E present \in BOOLEAN : ProjectRealtimeIntentStopped(present)
    \/ BeginRealtimeBindingIdle
    \/ BeginRealtimeBindingAttached
    \/ BeginRealtimeBindingRunning
    \/ BeginRealtimeBindingRetired
    \/ BeginRealtimeBindingStopped
    \/ ReplaceRealtimeBindingIdle
    \/ ReplaceRealtimeBindingAttached
    \/ ReplaceRealtimeBindingRunning
    \/ ReplaceRealtimeBindingRetired
    \/ ReplaceRealtimeBindingStopped
    \/ DetachRealtimeBindingIdle
    \/ DetachRealtimeBindingAttached
    \/ DetachRealtimeBindingRunning
    \/ DetachRealtimeBindingRetired
    \/ DetachRealtimeBindingStopped
    \/ RequireRealtimeReattachIdle
    \/ RequireRealtimeReattachAttached
    \/ RequireRealtimeReattachRunning
    \/ RequireRealtimeReattachRetired
    \/ RequireRealtimeReattachStopped
    \/ \E authority_epoch \in 0..2 : \E next_binding_state \in RealtimeBindingStateValues : PublishRealtimeSignalIdle(authority_epoch, next_binding_state)
    \/ \E authority_epoch \in 0..2 : \E next_binding_state \in RealtimeBindingStateValues : PublishRealtimeSignalAttached(authority_epoch, next_binding_state)
    \/ \E authority_epoch \in 0..2 : \E next_binding_state \in RealtimeBindingStateValues : PublishRealtimeSignalRunning(authority_epoch, next_binding_state)
    \/ \E authority_epoch \in 0..2 : \E next_binding_state \in RealtimeBindingStateValues : PublishRealtimeSignalRetired(authority_epoch, next_binding_state)
    \/ \E authority_epoch \in 0..2 : \E next_binding_state \in RealtimeBindingStateValues : PublishRealtimeSignalStopped(authority_epoch, next_binding_state)
    \/ \E server_id \in McpServerIdValues : McpServerConnectPendingIdle(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectPendingAttached(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectPendingRunning(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectPendingRetired(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectPendingStopped(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectedIdle(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectedAttached(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectedRunning(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectedRetired(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerConnectedStopped(server_id)
    \/ \E server_id \in McpServerIdValues : \E error \in StringValues : McpServerFailedIdle(server_id, error)
    \/ \E server_id \in McpServerIdValues : \E error \in StringValues : McpServerFailedAttached(server_id, error)
    \/ \E server_id \in McpServerIdValues : \E error \in StringValues : McpServerFailedRunning(server_id, error)
    \/ \E server_id \in McpServerIdValues : \E error \in StringValues : McpServerFailedRetired(server_id, error)
    \/ \E server_id \in McpServerIdValues : \E error \in StringValues : McpServerFailedStopped(server_id, error)
    \/ \E server_id \in McpServerIdValues : McpServerDisconnectedIdle(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerDisconnectedAttached(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerDisconnectedRunning(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerDisconnectedRetired(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerDisconnectedStopped(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerReloadIdle(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerReloadAttached(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerReloadRunning(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerReloadRetired(server_id)
    \/ \E server_id \in McpServerIdValues : McpServerReloadStopped(server_id)
    \/ \E corr_id \in PeerCorrelationIdValues : \E to \in StringValues : PeerRequestSentIdle(corr_id, to)
    \/ \E corr_id \in PeerCorrelationIdValues : \E to \in StringValues : PeerRequestSentAttached(corr_id, to)
    \/ \E corr_id \in PeerCorrelationIdValues : \E to \in StringValues : PeerRequestSentRunning(corr_id, to)
    \/ \E corr_id \in PeerCorrelationIdValues : \E to \in StringValues : PeerRequestSentRetired(corr_id, to)
    \/ \E corr_id \in PeerCorrelationIdValues : \E to \in StringValues : PeerRequestSentStopped(corr_id, to)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseProgressArrivedIdle(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseProgressArrivedAttached(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseProgressArrivedRunning(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseProgressArrivedRetired(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseProgressArrivedStopped(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedCompletedIdle(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedCompletedAttached(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedCompletedRunning(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedCompletedRetired(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedCompletedStopped(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedFailedIdle(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedFailedAttached(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedFailedRunning(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedFailedRetired(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : \E disposition \in PeerTerminalDispositionValues : PeerResponseTerminalArrivedFailedStopped(corr_id, disposition)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestTimedOutIdle(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestTimedOutAttached(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestTimedOutRunning(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestTimedOutRetired(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestTimedOutStopped(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestReceivedIdle(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestReceivedAttached(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestReceivedRunning(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestReceivedRetired(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerRequestReceivedStopped(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseRepliedIdle(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseRepliedAttached(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseRepliedRunning(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseRepliedRetired(corr_id)
    \/ \E corr_id \in PeerCorrelationIdValues : PeerResponseRepliedStopped(corr_id)
    \/ \E updated_at_ms \in 0..2 : AdvanceSessionContextIdle(updated_at_ms)
    \/ \E updated_at_ms \in 0..2 : AdvanceSessionContextAttached(updated_at_ms)
    \/ \E updated_at_ms \in 0..2 : AdvanceSessionContextRunning(updated_at_ms)
    \/ \E updated_at_ms \in 0..2 : AdvanceSessionContextRetired(updated_at_ms)
    \/ \E updated_at_ms \in 0..2 : AdvanceSessionContextStopped(updated_at_ms)
    \/ \E authority_epoch \in 0..2 : BeginLiveTopologyReconfigureIdle(authority_epoch)
    \/ \E authority_epoch \in 0..2 : BeginLiveTopologyReconfigureAttached(authority_epoch)
    \/ \E authority_epoch \in 0..2 : BeginLiveTopologyReconfigureRunning(authority_epoch)
    \/ \E authority_epoch \in 0..2 : BeginLiveTopologyReconfigureRetired(authority_epoch)
    \/ \E authority_epoch \in 0..2 : BeginLiveTopologyReconfigureStopped(authority_epoch)
    \/ MarkLiveTopologyDetachedIdle
    \/ MarkLiveTopologyDetachedAttached
    \/ MarkLiveTopologyDetachedRunning
    \/ MarkLiveTopologyDetachedRetired
    \/ MarkLiveTopologyDetachedStopped
    \/ ApplyLiveTopologyIdentityIdle
    \/ ApplyLiveTopologyIdentityAttached
    \/ ApplyLiveTopologyIdentityRunning
    \/ ApplyLiveTopologyIdentityRetired
    \/ ApplyLiveTopologyIdentityStopped
    \/ ApplyLiveTopologyVisibilityIdle
    \/ ApplyLiveTopologyVisibilityAttached
    \/ ApplyLiveTopologyVisibilityRunning
    \/ ApplyLiveTopologyVisibilityRetired
    \/ ApplyLiveTopologyVisibilityStopped
    \/ CompleteLiveTopologyIdle
    \/ CompleteLiveTopologyAttached
    \/ CompleteLiveTopologyRunning
    \/ CompleteLiveTopologyRetired
    \/ CompleteLiveTopologyStopped
    \/ AbortLiveTopologyBeforeDetachIdle
    \/ AbortLiveTopologyBeforeDetachAttached
    \/ AbortLiveTopologyBeforeDetachRunning
    \/ AbortLiveTopologyBeforeDetachRetired
    \/ AbortLiveTopologyBeforeDetachStopped
    \/ FailLiveTopologyAfterDetachIdle
    \/ FailLiveTopologyAfterDetachAttached
    \/ FailLiveTopologyAfterDetachRunning
    \/ FailLiveTopologyAfterDetachRetired
    \/ FailLiveTopologyAfterDetachStopped
    \/ TerminalStutter

fence_requires_bound_runtime == ((active_fence_token = None) \/ (active_runtime_id # None))
running_has_current_run == ((phase # "Running") \/ (current_run_id # None))
current_run_only_while_running_or_retired == ((current_run_id = None) \/ (phase = "Running") \/ (phase = "Retired"))
realtime_binding_epoch_consistency == ((realtime_binding_state = "Unbound") = (realtime_binding_authority_epoch = None))

CiStateConstraint == /\ model_step_count <= 6 /\ Cardinality(silent_intent_overrides) <= 1 /\ Cardinality(DOMAIN mcp_server_states) <= 1 /\ Cardinality(DOMAIN pending_peer_requests) <= 1 /\ Cardinality(DOMAIN inbound_peer_requests) <= 1
DeepStateConstraint == /\ model_step_count <= 8 /\ Cardinality(silent_intent_overrides) <= 2 /\ Cardinality(DOMAIN mcp_server_states) <= 2 /\ Cardinality(DOMAIN pending_peer_requests) <= 2 /\ Cardinality(DOMAIN inbound_peer_requests) <= 2

Spec == Init /\ [][Next]_vars

THEOREM Spec => []fence_requires_bound_runtime
THEOREM Spec => []running_has_current_run
THEOREM Spec => []current_run_only_while_running_or_retired
THEOREM Spec => []realtime_binding_epoch_consistency

=============================================================================
