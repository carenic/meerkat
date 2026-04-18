---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for MeerkatMachine.

CONSTANTS AgentRuntimeIdValues, BooleanValues, FenceTokenValues, GenerationValues, InputIdValues, NatValues, RunIdValues, SessionIdValues, SessionLlmCapabilitySurfaceStatusValues, SessionLlmCapabilitySurfaceValues, SessionLlmIdentityValues, SessionToolVisibilityDeltaValues, SessionToolVisibilityStateValues, SetOfStringValues, StringValues, ToolFilterValues, ToolVisibilityWitnessValues, WorkIdValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

OptionAgentRuntimeIdValues == {None} \cup {Some(x) : x \in AgentRuntimeIdValues}
OptionFenceTokenValues == {None} \cup {Some(x) : x \in FenceTokenValues}
OptionRunIdValues == {None} \cup {Some(x) : x \in RunIdValues}
OptionSessionIdValues == {None} \cup {Some(x) : x \in SessionIdValues}
OptionSessionLlmCapabilitySurfaceValues == {None} \cup {Some(x) : x \in SessionLlmCapabilitySurfaceValues}
OptionStringValues == {None} \cup {Some(x) : x \in StringValues}
MapStringToolVisibilityWitnessValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in ToolVisibilityWitnessValues }
MapStringU64Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in NatValues }

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
MapRemove(map, key) == [x \in DOMAIN map \ {key} |-> map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN SeqRemove(Tail(seq), value) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))

VARIABLES phase, model_step_count, session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt

vars == << phase, model_step_count, session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>

Init ==
    /\ phase = "Initializing"
    /\ model_step_count = 0
    /\ session_id = None
    /\ active_runtime_id = None
    /\ active_fence_token = None
    /\ current_run_id = None
    /\ pre_run_phase = None
    /\ silent_intent_overrides = {}
    /\ auth_valid_leases = {}
    /\ auth_expiring_leases = {}
    /\ auth_refreshing_leases = {}
    /\ auth_reauth_required_leases = {}
    /\ auth_expires_at = [x \in {} |-> None]
    /\ auth_last_refresh = [x \in {} |-> None]
    /\ auth_refresh_attempt = [x \in {} |-> None]

TerminalStutter ==
    /\ phase = "Destroyed"
    /\ UNCHANGED vars

Initialize ==
    /\ phase = "Initializing"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RegisterSessionIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RegisterSessionAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RegisterSessionRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RegisterSessionRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RegisterSessionStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ session_id' = Some(arg_session_id)
    /\ UNCHANGED << active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


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
    /\ UNCHANGED << silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


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
    /\ UNCHANGED << silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


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
    /\ UNCHANGED << silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


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
    /\ UNCHANGED << silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


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
    /\ UNCHANGED << silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ReconfigureSessionLlmIdentityAttached(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (active_runtime_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ReconfigureSessionLlmIdentityRunning(previous_identity, previous_visibility_state, previous_capability_surface, previous_capability_surface_status, target_identity, target_capability_surface, next_visibility_state, next_capability_base_filter, next_active_visibility_revision, tool_visibility_delta) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (active_runtime_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StagePersistentFilterIdle(filter, witnesses) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StagePersistentFilterAttached(filter, witnesses) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StagePersistentFilterRunning(filter, witnesses) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StagePersistentFilterRetired(filter, witnesses) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StagePersistentFilterStopped(filter, witnesses) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RequestDeferredToolsIdle(names, witnesses) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RequestDeferredToolsAttached(names, witnesses) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RequestDeferredToolsRunning(names, witnesses) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RequestDeferredToolsRetired(names, witnesses) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RequestDeferredToolsStopped(names, witnesses) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsInitializing(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Initializing"
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsIdle(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Idle"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsAttached(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsRunning(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsRetired(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareBindingsStopped(agent_runtime_id, fence_token, generation) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ active_runtime_id' = Some(agent_runtime_id)
    /\ active_fence_token' = Some(fence_token)
    /\ UNCHANGED << session_id, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetPeerIngressContextIdle(keep_alive) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetPeerIngressContextAttached(keep_alive) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetPeerIngressContextRunning(keep_alive) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetPeerIngressContextRetired(keep_alive) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetPeerIngressContextStopped(keep_alive) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


NotifyDrainExitedIdle(reason) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


NotifyDrainExitedAttached(reason) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


NotifyDrainExitedRunning(reason) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


NotifyDrainExitedRetired(reason) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


NotifyDrainExitedStopped(reason) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


InterruptCurrentRunAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


InterruptCurrentRun ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CancelAfterBoundaryAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CancelAfterBoundary ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BoundaryAppliedPublish(revision) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishCommittedVisibleSetIdle(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishCommittedVisibleSetAttached(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishCommittedVisibleSetRunning(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishCommittedVisibleSetRetired(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishCommittedVisibleSetStopped(active_filter, staged_filter, active_requested_deferred_names, staged_requested_deferred_names, active_visibility_revision, staged_visibility_revision) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ (active_visibility_revision >= staged_visibility_revision)
    /\ ((active_visibility_revision # staged_visibility_revision) \/ ((active_filter = staged_filter) /\ (active_requested_deferred_names = staged_requested_deferred_names)))
    /\ (\A requested_name \in active_requested_deferred_names : (requested_name \in staged_requested_deferred_names))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RetireRequestedFromIdle ==
    /\ phase = "Idle" \/ phase = "Attached" \/ phase = "Running"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


Reset ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Attached" \/ phase = "Retired"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StopRuntimeExecutorUnbound ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Retired"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StopRuntimeExecutorAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StopRuntimeExecutorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


Destroy ==
    /\ phase = "Initializing" \/ phase = "Idle" \/ phase = "Attached" \/ phase = "Running" \/ phase = "Retired" \/ phase = "Stopped"
    /\ (active_runtime_id # None)
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ silent_intent_overrides' = {}
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureSessionWithExecutorIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureSessionWithExecutorAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureSessionWithExecutorRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureSessionWithExecutorRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureSessionWithExecutorStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetSilentIntentsIdle(arg_session_id, intents) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetSilentIntentsAttached(arg_session_id, intents) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetSilentIntentsRunning(arg_session_id, intents) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetSilentIntentsRetired(arg_session_id, intents) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ silent_intent_overrides' = intents
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SetSilentIntentsStopped(arg_session_id, intents) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


WaitIdle(arg_session_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


WaitAttached(arg_session_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


WaitRunning(arg_session_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


WaitRetired(arg_session_id) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


WaitStopped(arg_session_id) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAllIdle ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAllAttached ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAllRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAllRetired ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AbortAllStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureDrainRunningAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


EnsureDrainRunningRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


IngestIdle(runtime_id, work_id, origin) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


IngestAttached(runtime_id, work_id, origin) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


IngestRunning(runtime_id, work_id, origin) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishEventIdle(kind) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishEventAttached(kind) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishEventRunning(kind) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishEventRetired(kind) ==
    /\ phase = "Retired"
    /\ (session_id # None)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PublishEventStopped(kind) ==
    /\ phase = "Stopped"
    /\ (session_id # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionIdleQueued(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionIdleImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionAttachedImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("attached")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionAttachedQueued(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionRunningQueuedPassive(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionRunningInterruptYielding(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = FALSE)
    /\ (interrupt_yielding = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithCompletionRunningImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ (request_immediate_processing = TRUE)
    /\ (interrupt_yielding = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithoutWakeIdle(input_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithoutWakeAttached(input_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcceptWithoutWakeRunning(input_id) ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ClassifyExternalEnvelopeAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ClassifyExternalEnvelopeRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ClassifyPlainEventAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ClassifyPlainEventRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareIdle(arg_session_id, run_id) ==
    /\ phase = "Idle"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("idle")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PrepareAttached(arg_session_id, run_id) ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("attached")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


DrainQueuedRunRetired(run_id) ==
    /\ phase = "Retired"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = Some(run_id)
    /\ pre_run_phase' = Some("retired")
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StartConversationRunAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StartImmediateAppendAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StartImmediateContextAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CommitRunningToIdle(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("idle"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CommitRunningToAttached(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("attached"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CommitRunningToRetired(input_id, run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("retired"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FailRunningToIdle(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("idle"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FailRunningToAttached(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("attached"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FailRunningToRetired(run_id) ==
    /\ phase = "Running"
    /\ (pre_run_phase = Some("retired"))
    /\ (current_run_id = Some(run_id))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ current_run_id' = None
    /\ pre_run_phase' = None
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageAddAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageAddRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageRemoveAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageRemoveRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageReloadAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


StageReloadRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ApplySurfaceBoundaryAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ApplySurfaceBoundaryRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PendingSucceededAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PendingSucceededRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PendingFailedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


PendingFailedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CallStartedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CallStartedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CallFinishedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CallFinishedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FinalizeRemovalCleanAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FinalizeRemovalCleanRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FinalizeRemovalForcedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


FinalizeRemovalForcedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SnapshotAlignedAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


SnapshotAlignedRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ShutdownSurfaceAttached ==
    /\ phase = "Attached"
    /\ (session_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ShutdownSurfaceRunning ==
    /\ phase = "Running"
    /\ (session_id # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RecycleFromIdleOrRetired ==
    /\ phase = "Idle" \/ phase = "Retired"
    /\ (active_runtime_id # None)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ UNCHANGED << session_id, active_runtime_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


RecycleFromAttached ==
    /\ phase = "Attached"
    /\ (active_runtime_id # None)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ active_fence_token' = None
    /\ current_run_id' = None
    /\ UNCHANGED << session_id, active_runtime_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseInitializing(binding_key, expires_at) ==
    /\ phase = "Initializing"
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseIdle(binding_key, expires_at) ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseAttached(binding_key, expires_at) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseRunning(binding_key, expires_at) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseRetired(binding_key, expires_at) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


AcquireAuthLeaseStopped(binding_key, expires_at) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, expires_at)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringInitializing(binding_key) ==
    /\ phase = "Initializing"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringIdle(binding_key) ==
    /\ phase = "Idle"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringAttached(binding_key) ==
    /\ phase = "Attached"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringRunning(binding_key) ==
    /\ phase = "Running"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringRetired(binding_key) ==
    /\ phase = "Retired"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkAuthExpiringStopped(binding_key) ==
    /\ phase = "Stopped"
    /\ (binding_key \in auth_valid_leases)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_refreshing_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshInitializing(binding_key) ==
    /\ phase = "Initializing"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshIdle(binding_key) ==
    /\ phase = "Idle"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshAttached(binding_key) ==
    /\ phase = "Attached"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshRunning(binding_key) ==
    /\ phase = "Running"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshRetired(binding_key) ==
    /\ phase = "Retired"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


BeginAuthRefreshStopped(binding_key) ==
    /\ phase = "Stopped"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_reauth_required_leases, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


CompleteAuthRefreshInitializing(binding_key, new_expires_at, now) ==
    /\ phase = "Initializing"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


CompleteAuthRefreshIdle(binding_key, new_expires_at, now) ==
    /\ phase = "Idle"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


CompleteAuthRefreshAttached(binding_key, new_expires_at, now) ==
    /\ phase = "Attached"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


CompleteAuthRefreshRunning(binding_key, new_expires_at, now) ==
    /\ phase = "Running"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


CompleteAuthRefreshRetired(binding_key, new_expires_at, now) ==
    /\ phase = "Retired"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


CompleteAuthRefreshStopped(binding_key, new_expires_at, now) ==
    /\ phase = "Stopped"
    /\ (binding_key \in auth_refreshing_leases)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_expires_at' = MapSet(auth_expires_at, binding_key, new_expires_at)
    /\ auth_last_refresh' = MapSet(auth_last_refresh, binding_key, now)
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 0)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expiring_leases, auth_reauth_required_leases >>


AuthRefreshFailedTransientInitializing(binding_key, permanent) ==
    /\ phase = "Initializing"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedTransientIdle(binding_key, permanent) ==
    /\ phase = "Idle"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedTransientAttached(binding_key, permanent) ==
    /\ phase = "Attached"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedTransientRunning(binding_key, permanent) ==
    /\ phase = "Running"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedTransientRetired(binding_key, permanent) ==
    /\ phase = "Retired"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedTransientStopped(binding_key, permanent) ==
    /\ phase = "Stopped"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = FALSE)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_expiring_leases' = (auth_expiring_leases \cup {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_reauth_required_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentInitializing(binding_key, permanent) ==
    /\ phase = "Initializing"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentIdle(binding_key, permanent) ==
    /\ phase = "Idle"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentAttached(binding_key, permanent) ==
    /\ phase = "Attached"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentRunning(binding_key, permanent) ==
    /\ phase = "Running"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentRetired(binding_key, permanent) ==
    /\ phase = "Retired"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


AuthRefreshFailedPermanentStopped(binding_key, permanent) ==
    /\ phase = "Stopped"
    /\ (binding_key \in auth_refreshing_leases)
    /\ (permanent = TRUE)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ auth_refresh_attempt' = MapSet(auth_refresh_attempt, binding_key, 1)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_valid_leases, auth_expiring_leases, auth_expires_at, auth_last_refresh >>


MarkReauthRequiredInitializing(binding_key) ==
    /\ phase = "Initializing"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkReauthRequiredIdle(binding_key) ==
    /\ phase = "Idle"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkReauthRequiredAttached(binding_key) ==
    /\ phase = "Attached"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkReauthRequiredRunning(binding_key) ==
    /\ phase = "Running"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkReauthRequiredRetired(binding_key) ==
    /\ phase = "Retired"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


MarkReauthRequiredStopped(binding_key) ==
    /\ phase = "Stopped"
    /\ ((binding_key \in auth_valid_leases) \/ (binding_key \in auth_expiring_leases) \/ (binding_key \in auth_refreshing_leases))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \cup {binding_key})
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides, auth_expires_at, auth_last_refresh, auth_refresh_attempt >>


ReleaseAuthLeaseInitializing(binding_key) ==
    /\ phase = "Initializing"
    /\ phase' = "Initializing"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


ReleaseAuthLeaseIdle(binding_key) ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


ReleaseAuthLeaseAttached(binding_key) ==
    /\ phase = "Attached"
    /\ phase' = "Attached"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


ReleaseAuthLeaseRunning(binding_key) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


ReleaseAuthLeaseRetired(binding_key) ==
    /\ phase = "Retired"
    /\ phase' = "Retired"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


ReleaseAuthLeaseStopped(binding_key) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ auth_valid_leases' = (auth_valid_leases \ {binding_key})
    /\ auth_expiring_leases' = (auth_expiring_leases \ {binding_key})
    /\ auth_refreshing_leases' = (auth_refreshing_leases \ {binding_key})
    /\ auth_reauth_required_leases' = (auth_reauth_required_leases \ {binding_key})
    /\ auth_expires_at' = MapRemove(auth_expires_at, binding_key)
    /\ auth_last_refresh' = MapRemove(auth_last_refresh, binding_key)
    /\ auth_refresh_attempt' = MapRemove(auth_refresh_attempt, binding_key)
    /\ UNCHANGED << session_id, active_runtime_id, active_fence_token, current_run_id, pre_run_phase, silent_intent_overrides >>


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
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionIdleQueued(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionIdleImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionAttachedImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionAttachedQueued(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningQueuedPassive(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningInterruptYielding(input_id, request_immediate_processing, interrupt_yielding, run_id)
    \/ \E input_id \in InputIdValues : \E request_immediate_processing \in BOOLEAN : \E interrupt_yielding \in BOOLEAN : \E run_id \in RunIdValues : AcceptWithCompletionRunningImmediate(input_id, request_immediate_processing, interrupt_yielding, run_id)
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
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseInitializing(binding_key, expires_at)
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseIdle(binding_key, expires_at)
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseAttached(binding_key, expires_at)
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseRunning(binding_key, expires_at)
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseRetired(binding_key, expires_at)
    \/ \E binding_key \in StringValues : \E expires_at \in 0..2 : AcquireAuthLeaseStopped(binding_key, expires_at)
    \/ \E binding_key \in StringValues : MarkAuthExpiringInitializing(binding_key)
    \/ \E binding_key \in StringValues : MarkAuthExpiringIdle(binding_key)
    \/ \E binding_key \in StringValues : MarkAuthExpiringAttached(binding_key)
    \/ \E binding_key \in StringValues : MarkAuthExpiringRunning(binding_key)
    \/ \E binding_key \in StringValues : MarkAuthExpiringRetired(binding_key)
    \/ \E binding_key \in StringValues : MarkAuthExpiringStopped(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshInitializing(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshIdle(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshAttached(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshRunning(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshRetired(binding_key)
    \/ \E binding_key \in StringValues : BeginAuthRefreshStopped(binding_key)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshInitializing(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshIdle(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshAttached(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshRunning(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshRetired(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E new_expires_at \in 0..2 : \E now \in 0..2 : CompleteAuthRefreshStopped(binding_key, new_expires_at, now)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientInitializing(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientIdle(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientAttached(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientRunning(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientRetired(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedTransientStopped(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentInitializing(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentIdle(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentAttached(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentRunning(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentRetired(binding_key, permanent)
    \/ \E binding_key \in StringValues : \E permanent \in BOOLEAN : AuthRefreshFailedPermanentStopped(binding_key, permanent)
    \/ \E binding_key \in StringValues : MarkReauthRequiredInitializing(binding_key)
    \/ \E binding_key \in StringValues : MarkReauthRequiredIdle(binding_key)
    \/ \E binding_key \in StringValues : MarkReauthRequiredAttached(binding_key)
    \/ \E binding_key \in StringValues : MarkReauthRequiredRunning(binding_key)
    \/ \E binding_key \in StringValues : MarkReauthRequiredRetired(binding_key)
    \/ \E binding_key \in StringValues : MarkReauthRequiredStopped(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseInitializing(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseIdle(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseAttached(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseRunning(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseRetired(binding_key)
    \/ \E binding_key \in StringValues : ReleaseAuthLeaseStopped(binding_key)
    \/ TerminalStutter

fence_requires_bound_runtime == ((active_fence_token = None) \/ (active_runtime_id # None))
running_has_current_run == ((phase # "Running") \/ (current_run_id # None))
current_run_only_while_running_or_retired == ((current_run_id = None) \/ (phase = "Running") \/ (phase = "Retired"))
auth_state_is_exclusive == ((\A k \in auth_valid_leases : (~((k \in auth_expiring_leases)) /\ ~((k \in auth_refreshing_leases)) /\ ~((k \in auth_reauth_required_leases)))) /\ (\A k \in auth_expiring_leases : (~((k \in auth_refreshing_leases)) /\ ~((k \in auth_reauth_required_leases)))) /\ (\A k \in auth_refreshing_leases : ~((k \in auth_reauth_required_leases))))

CiStateConstraint == /\ model_step_count <= 6 /\ Cardinality(silent_intent_overrides) <= 1 /\ Cardinality(auth_valid_leases) <= 1 /\ Cardinality(auth_expiring_leases) <= 1 /\ Cardinality(auth_refreshing_leases) <= 1 /\ Cardinality(auth_reauth_required_leases) <= 1 /\ Cardinality(DOMAIN auth_expires_at) <= 1 /\ Cardinality(DOMAIN auth_last_refresh) <= 1 /\ Cardinality(DOMAIN auth_refresh_attempt) <= 1
DeepStateConstraint == /\ model_step_count <= 8 /\ Cardinality(silent_intent_overrides) <= 2 /\ Cardinality(auth_valid_leases) <= 2 /\ Cardinality(auth_expiring_leases) <= 2 /\ Cardinality(auth_refreshing_leases) <= 2 /\ Cardinality(auth_reauth_required_leases) <= 2 /\ Cardinality(DOMAIN auth_expires_at) <= 2 /\ Cardinality(DOMAIN auth_last_refresh) <= 2 /\ Cardinality(DOMAIN auth_refresh_attempt) <= 2

Spec == Init /\ [][Next]_vars

THEOREM Spec => []fence_requires_bound_runtime
THEOREM Spec => []running_has_current_run
THEOREM Spec => []current_run_only_while_running_or_retired
THEOREM Spec => []auth_state_is_exclusive

=============================================================================
