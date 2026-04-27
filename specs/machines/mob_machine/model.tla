---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for MobMachine.

CONSTANTS AgentIdentityValues, AgentRuntimeIdValues, BooleanValues, BranchIdValues, CollectionPolicyKindValues, DependencyModeValues, ExternalPeerEdgeValues, FenceTokenValues, FlowFrameReducerCommandKindValues, FlowNodeIdValues, FlowNodeKindValues, FlowRunReducerCommandKindValues, FlowRunStatusValues, FrameIdValues, FrameScopeValues, FrameStatusValues, GenerationValues, LoopIdValues, LoopInstanceIdValues, LoopIterationReducerCommandKindValues, LoopIterationStageValues, LoopStatusValues, MobIdValues, MobMemberStateValues, MobTaskValues, NatValues, NodeRunStatusValues, RunIdValues, RunStepKeyValues, SessionIdValues, SetOfAgentIdentityValues, SetOfAgentRuntimeIdValues, SetOfExternalPeerEdgeValues, SetOfFlowNodeIdValues, SetOfFrameIdValues, SetOfLoopInstanceIdValues, SetOfStepIdValues, SetOfStringValues, SetOfTaskIdValues, SetOfWiringEdgeValues, StepIdValues, StepRunStatusValues, StringValues, TaskIdValues, TaskStatusValues, WiringEdgeValues, WorkIdValues, WorkOriginValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

MapAgentIdentityAgentRuntimeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in AgentIdentityValues, v \in AgentRuntimeIdValues }
MapAgentIdentitySessionIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in AgentIdentityValues, v \in SessionIdValues }
MapAgentRuntimeIdFenceTokenValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in AgentRuntimeIdValues, v \in FenceTokenValues }
MapAgentRuntimeIdMobMemberStateValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in AgentRuntimeIdValues, v \in MobMemberStateValues }
MapFlowNodeIdBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in BOOLEAN }
MapFlowNodeIdDependencyModeValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in DependencyModeValues }
MapFlowNodeIdFlowNodeKindValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in FlowNodeKindValues }
MapFlowNodeIdLoopIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in LoopIdValues }
MapFlowNodeIdNodeRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in NodeRunStatusValues }
MapFlowNodeIdStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in StepIdValues }
MapFrameIdFrameScopeValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in FrameScopeValues }
MapFrameIdFrameStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in FrameStatusValues }
MapFrameIdRunIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in RunIdValues }
MapFrameIdU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in NatValues }
MapLoopInstanceIdFlowNodeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in FlowNodeIdValues }
MapLoopInstanceIdFrameIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in FrameIdValues }
MapLoopInstanceIdLoopIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in LoopIdValues }
MapLoopInstanceIdLoopIterationStageValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in LoopIterationStageValues }
MapLoopInstanceIdLoopStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in LoopStatusValues }
MapLoopInstanceIdU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in NatValues }
MapLoopInstanceIdU64Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in NatValues }
MapRunIdFlowRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in FlowRunStatusValues }
MapRunIdU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in NatValues }
MapRunStepKeyBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunStepKeyValues, v \in BOOLEAN }
MapRunStepKeyStepRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunStepKeyValues, v \in StepRunStatusValues }
MapStepIdBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in BOOLEAN }
MapStepIdCollectionPolicyKindValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in CollectionPolicyKindValues }
MapStepIdDependencyModeValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in DependencyModeValues }
MapStepIdU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in NatValues }
MapStringStringValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in StringValues }
MapStringU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in NatValues }
MapTaskIdMobTaskValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in TaskIdValues, v \in MobTaskValues }
OptionBoolValues == {None} \cup {Some(x) : x \in BOOLEAN}
OptionBranchIdValues == {None} \cup {Some(x) : x \in BranchIdValues}
OptionFlowNodeIdValues == {None} \cup {Some(x) : x \in FlowNodeIdValues}
OptionFrameIdValues == {None} \cup {Some(x) : x \in FrameIdValues}
OptionFrameStatusValues == {None} \cup {Some(x) : x \in FrameStatusValues}
OptionLoopInstanceIdValues == {None} \cup {Some(x) : x \in LoopInstanceIdValues}
OptionNodeRunStatusValues == {None} \cup {Some(x) : x \in NodeRunStatusValues}
OptionRunStepKeyValues == {None} \cup {Some(x) : x \in RunStepKeyValues}
OptionSessionIdValues == {None} \cup {Some(x) : x \in SessionIdValues}
OptionStepIdValues == {None} \cup {Some(x) : x \in StepIdValues}
OptionStepRunStatusValues == {None} \cup {Some(x) : x \in StepRunStatusValues}
OptionStringValues == {None} \cup {Some(x) : x \in StringValues}
OptionU32Values == {None} \cup {Some(x) : x \in NatValues}
OptionU64Values == {None} \cup {Some(x) : x \in NatValues}
SeqOfFlowNodeIdValues == {<<>>} \cup {<<x>> : x \in FlowNodeIdValues} \cup {<<x, y>> : x \in FlowNodeIdValues, y \in FlowNodeIdValues}
SeqOfFrameIdValues == {<<>>} \cup {<<x>> : x \in FrameIdValues} \cup {<<x, y>> : x \in FrameIdValues, y \in FrameIdValues}
SeqOfLoopInstanceIdValues == {<<>>} \cup {<<x>> : x \in LoopInstanceIdValues} \cup {<<x, y>> : x \in LoopInstanceIdValues, y \in LoopInstanceIdValues}
SeqOfStepIdValues == {<<>>} \cup {<<x>> : x \in StepIdValues} \cup {<<x, y>> : x \in StepIdValues, y \in StepIdValues}
MapFlowNodeIdOptionBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in OptionBoolValues }
MapFlowNodeIdOptionBranchIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in OptionBranchIdValues }
MapFlowNodeIdSeqFlowNodeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FlowNodeIdValues, v \in SeqOfFlowNodeIdValues }
MapFrameIdMapFlowNodeIdBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdBoolValues }
MapFrameIdMapFlowNodeIdDependencyModeValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdDependencyModeValues }
MapFrameIdMapFlowNodeIdFlowNodeKindValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdFlowNodeKindValues }
MapFrameIdMapFlowNodeIdLoopIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdLoopIdValues }
MapFrameIdMapFlowNodeIdNodeRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdNodeRunStatusValues }
MapFrameIdMapFlowNodeIdStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdStepIdValues }
MapFrameIdOptionLoopInstanceIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in OptionLoopInstanceIdValues }
MapFrameIdSeqFlowNodeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in SeqOfFlowNodeIdValues }
MapFrameIdSetFlowNodeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in SetOfFlowNodeIdValues }
MapLoopInstanceIdOptionFrameIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in LoopInstanceIdValues, v \in OptionFrameIdValues }
MapRunIdMapStepIdBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdBoolValues }
MapRunIdMapStepIdCollectionPolicyKindValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdCollectionPolicyKindValues }
MapRunIdMapStepIdDependencyModeValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdDependencyModeValues }
MapRunIdMapStepIdU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdU32Values }
MapRunIdMapStringU32Values == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStringU32Values }
MapRunIdSeqFrameIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SeqOfFrameIdValues }
MapRunIdSeqLoopInstanceIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SeqOfLoopInstanceIdValues }
MapRunIdSeqStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SeqOfStepIdValues }
MapRunIdSetFrameIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SetOfFrameIdValues }
MapRunIdSetLoopInstanceIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SetOfLoopInstanceIdValues }
MapRunIdSetStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in SetOfStepIdValues }
MapStepIdOptionBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in OptionBoolValues }
MapStepIdOptionBranchIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in OptionBranchIdValues }
MapStepIdOptionStepRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in OptionStepRunStatusValues }
MapStepIdSeqStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StepIdValues, v \in SeqOfStepIdValues }
OptionSeqFlowNodeIdValues == {None} \cup {Some(x) : x \in SeqOfFlowNodeIdValues}
MapFrameIdMapFlowNodeIdOptionBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdOptionBoolValues }
MapFrameIdMapFlowNodeIdOptionBranchIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdOptionBranchIdValues }
MapFrameIdMapFlowNodeIdSeqFlowNodeIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in FrameIdValues, v \in MapFlowNodeIdSeqFlowNodeIdValues }
MapRunIdMapStepIdOptionBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdOptionBoolValues }
MapRunIdMapStepIdOptionBranchIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdOptionBranchIdValues }
MapRunIdMapStepIdOptionStepRunStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdOptionStepRunStatusValues }
MapRunIdMapStepIdSeqStepIdValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in RunIdValues, v \in MapStepIdSeqStepIdValues }

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
MapIncrement(map, key, amount) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN (IF key \in DOMAIN map THEN map[key] ELSE 0) + amount ELSE map[x]]
MapDecrement(map, key, amount) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN (IF key \in DOMAIN map THEN map[key] ELSE 0) - amount ELSE map[x]]
MapRemove(map, key) == [x \in DOMAIN map \ {key} |-> map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN SeqRemove(Tail(seq), value) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))

VARIABLES phase, model_step_count, live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch

vars == << phase, model_step_count, live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>

Init ==
    /\ phase = "Running"
    /\ model_step_count = 0
    /\ live_runtime_ids = {}
    /\ externally_addressable_runtime_ids = {}
    /\ runtime_fence_tokens = [x \in {} |-> None]
    /\ active_run_count = 0
    /\ run_status = [x \in {} |-> None]
    /\ run_ordered_steps = [x \in {} |-> None]
    /\ run_tracked_steps = [x \in {} |-> None]
    /\ run_step_status = [x \in {} |-> None]
    /\ run_step_status_flat = [x \in {} |-> None]
    /\ run_output_recorded = [x \in {} |-> None]
    /\ run_step_condition_results = [x \in {} |-> None]
    /\ run_step_has_conditions = [x \in {} |-> None]
    /\ run_step_dependencies = [x \in {} |-> None]
    /\ run_step_dependency_modes = [x \in {} |-> None]
    /\ run_step_branches = [x \in {} |-> None]
    /\ run_step_collection_policies = [x \in {} |-> None]
    /\ run_step_quorum_thresholds = [x \in {} |-> None]
    /\ run_step_target_counts = [x \in {} |-> None]
    /\ run_step_target_success_counts = [x \in {} |-> None]
    /\ run_step_target_terminal_failure_counts = [x \in {} |-> None]
    /\ run_output_recorded_flat = [x \in {} |-> None]
    /\ run_target_retry_counts = [x \in {} |-> None]
    /\ run_escalation_threshold = [x \in {} |-> None]
    /\ run_max_step_retries = [x \in {} |-> None]
    /\ run_ready_frames = [x \in {} |-> None]
    /\ run_ready_frame_membership = [x \in {} |-> None]
    /\ run_pending_body_frame_loops = [x \in {} |-> None]
    /\ run_pending_body_frame_loop_membership = [x \in {} |-> None]
    /\ run_max_active_nodes = [x \in {} |-> None]
    /\ run_max_active_frames = [x \in {} |-> None]
    /\ run_max_frame_depth = [x \in {} |-> None]
    /\ frame_scope = [x \in {} |-> None]
    /\ frame_phase = [x \in {} |-> None]
    /\ frame_run = [x \in {} |-> None]
    /\ frame_parent_loop = [x \in {} |-> None]
    /\ frame_iteration = [x \in {} |-> None]
    /\ frame_tracked_nodes = [x \in {} |-> None]
    /\ frame_ordered_nodes = [x \in {} |-> None]
    /\ frame_node_kind = [x \in {} |-> None]
    /\ frame_node_dependencies = [x \in {} |-> None]
    /\ frame_node_dependency_modes = [x \in {} |-> None]
    /\ frame_node_step_ids = [x \in {} |-> None]
    /\ frame_node_loop_ids = [x \in {} |-> None]
    /\ frame_node_status = [x \in {} |-> None]
    /\ frame_ready_queue = [x \in {} |-> None]
    /\ frame_output_recorded = [x \in {} |-> None]
    /\ frame_node_condition_results = [x \in {} |-> None]
    /\ frame_node_branches = [x \in {} |-> None]
    /\ loop_phase = [x \in {} |-> None]
    /\ loop_parent_frame = [x \in {} |-> None]
    /\ loop_parent_node = [x \in {} |-> None]
    /\ loop_definition = [x \in {} |-> None]
    /\ loop_depth = [x \in {} |-> None]
    /\ loop_stage = [x \in {} |-> None]
    /\ loop_current_iteration = [x \in {} |-> None]
    /\ loop_last_completed_iteration = [x \in {} |-> None]
    /\ loop_max_iterations = [x \in {} |-> None]
    /\ loop_active_body_frame = [x \in {} |-> None]
    /\ pending_spawn_count = 0
    /\ pending_spawn_sessions = [x \in {} |-> None]
    /\ coordinator_bound = TRUE
    /\ member_startup_binding_requested = {}
    /\ member_startup_runtime_ready = {}
    /\ member_startup_ready = {}
    /\ member_kickoff_pending = {}
    /\ member_kickoff_starting = {}
    /\ member_kickoff_callback_pending = {}
    /\ member_kickoff_started = {}
    /\ member_kickoff_failed = {}
    /\ member_kickoff_cancelled = {}
    /\ member_kickoff_error = [x \in {} |-> None]
    /\ member_state_markers = [x \in {} |-> None]
    /\ wiring_edges = {}
    /\ external_peer_edges = {}
    /\ identity_to_runtime = [x \in {} |-> None]
    /\ tasks = [x \in {} |-> None]
    /\ in_progress_task_ids = {}
    /\ completed_task_ids = {}
    /\ member_session_bindings = [x \in {} |-> None]
    /\ pending_session_ingress_detach_runtime_ids = {}
    /\ topology_epoch = 0

TerminalStutter ==
    /\ phase = "Destroyed"
    /\ UNCHANGED vars

SpawnRunningFresh(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, bridge_session_id, replacing) ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = FALSE)
    /\ (replacing = None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = (live_runtime_ids \cup {agent_runtime_id})
    /\ externally_addressable_runtime_ids' = IF external_addressable THEN (externally_addressable_runtime_ids \cup {agent_runtime_id}) ELSE (externally_addressable_runtime_ids \ {agent_runtime_id})
    /\ runtime_fence_tokens' = MapSet(runtime_fence_tokens, agent_runtime_id, fence_token)
    /\ member_startup_binding_requested' = (member_startup_binding_requested \cup {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ identity_to_runtime' = MapSet(identity_to_runtime, agent_identity, agent_runtime_id)
    /\ member_session_bindings' = MapSet(member_session_bindings, agent_identity, bridge_session_id)
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, tasks, in_progress_task_ids, completed_task_ids, pending_session_ingress_detach_runtime_ids >>


SpawnRunningReplacing(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, bridge_session_id, replacing) ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ (replacing # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = (live_runtime_ids \cup {agent_runtime_id})
    /\ externally_addressable_runtime_ids' = IF external_addressable THEN (externally_addressable_runtime_ids \cup {agent_runtime_id}) ELSE (externally_addressable_runtime_ids \ {agent_runtime_id})
    /\ runtime_fence_tokens' = MapSet(runtime_fence_tokens, agent_runtime_id, fence_token)
    /\ member_startup_binding_requested' = (member_startup_binding_requested \cup {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ identity_to_runtime' = MapSet(identity_to_runtime, agent_identity, agent_runtime_id)
    /\ member_session_bindings' = MapSet(member_session_bindings, agent_identity, bridge_session_id)
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, tasks, in_progress_task_ids, completed_task_ids, pending_session_ingress_detach_runtime_ids >>


EnsureMemberRunningExisting(agent_identity) ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ ((agent_identity \in DOMAIN identity_to_runtime) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


EnsureMemberRunningMissing(agent_identity) ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ ((agent_identity \in DOMAIN identity_to_runtime) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ReconcileRunning(desired, retire_stale) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ReconcileStopped(desired, retire_stale) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ReconcileCompleted(desired, retire_stale) ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ObserveRuntimeReady(agent_runtime_id, fence_token) ==
    /\ phase = "Running"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_startup_binding_requested' = (member_startup_binding_requested \ {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \cup {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StartupMarkReadyRunning(agent_runtime_id, fence_token) ==
    /\ phase = "Running"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_startup_binding_requested' = (member_startup_binding_requested \ {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \cup {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StartupMarkReadyStopped(agent_runtime_id, fence_token) ==
    /\ phase = "Stopped"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_startup_binding_requested' = (member_startup_binding_requested \ {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \cup {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StartupMarkReadyCompleted(agent_runtime_id, fence_token) ==
    /\ phase = "Completed"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_startup_binding_requested' = (member_startup_binding_requested \ {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \cup {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkPendingRunning(member_id) ==
    /\ phase = "Running"
    /\ (~((member_id \in member_kickoff_pending)) /\ ~((member_id \in member_kickoff_starting)) /\ ~((member_id \in member_kickoff_callback_pending)) /\ ~((member_id \in member_kickoff_started)) /\ ~((member_id \in member_kickoff_failed)) /\ ~((member_id \in member_kickoff_cancelled)))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \cup {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkPendingStopped(member_id) ==
    /\ phase = "Stopped"
    /\ (~((member_id \in member_kickoff_pending)) /\ ~((member_id \in member_kickoff_starting)) /\ ~((member_id \in member_kickoff_callback_pending)) /\ ~((member_id \in member_kickoff_started)) /\ ~((member_id \in member_kickoff_failed)) /\ ~((member_id \in member_kickoff_cancelled)))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \cup {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkPendingCompleted(member_id) ==
    /\ phase = "Completed"
    /\ (~((member_id \in member_kickoff_pending)) /\ ~((member_id \in member_kickoff_starting)) /\ ~((member_id \in member_kickoff_callback_pending)) /\ ~((member_id \in member_kickoff_started)) /\ ~((member_id \in member_kickoff_failed)) /\ ~((member_id \in member_kickoff_cancelled)))
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \cup {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkStartingRunning(member_id) ==
    /\ phase = "Running"
    /\ (member_id \in member_kickoff_pending)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \cup {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkStartingStopped(member_id) ==
    /\ phase = "Stopped"
    /\ (member_id \in member_kickoff_pending)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \cup {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffMarkStartingCompleted(member_id) ==
    /\ phase = "Completed"
    /\ (member_id \in member_kickoff_pending)
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \cup {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveStartedRunning(member_id) ==
    /\ phase = "Running"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \cup {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveStartedStopped(member_id) ==
    /\ phase = "Stopped"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \cup {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveStartedCompleted(member_id) ==
    /\ phase = "Completed"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \cup {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCallbackPendingRunning(member_id) ==
    /\ phase = "Running"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \cup {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCallbackPendingStopped(member_id) ==
    /\ phase = "Stopped"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \cup {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCallbackPendingCompleted(member_id) ==
    /\ phase = "Completed"
    /\ (member_id \in member_kickoff_starting)
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \cup {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveFailedFromStartingRunning(member_id, error) ==
    /\ phase = "Running"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \cup {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapSet(member_kickoff_error, member_id, error)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveFailedFromStartingStopped(member_id, error) ==
    /\ phase = "Stopped"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \cup {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapSet(member_kickoff_error, member_id, error)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveFailedFromStartingCompleted(member_id, error) ==
    /\ phase = "Completed"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \cup {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapSet(member_kickoff_error, member_id, error)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCancelledRunning(member_id) ==
    /\ phase = "Running"
    /\ ~((member_id \in member_kickoff_started))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCancelledStopped(member_id) ==
    /\ phase = "Stopped"
    /\ ~((member_id \in member_kickoff_started))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffResolveCancelledCompleted(member_id) ==
    /\ phase = "Completed"
    /\ ~((member_id \in member_kickoff_started))
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffCancelRequestedRunning(member_id) ==
    /\ phase = "Running"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffCancelRequestedStopped(member_id) ==
    /\ phase = "Stopped"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffCancelRequestedCompleted(member_id) ==
    /\ phase = "Completed"
    /\ ((member_id \in member_kickoff_pending) \/ (member_id \in member_kickoff_starting) \/ (member_id \in member_kickoff_callback_pending))
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \cup {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffClearRunning(member_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffClearStopped(member_id) ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


KickoffClearCompleted(member_id) ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ member_kickoff_pending' = (member_kickoff_pending \ {member_id})
    /\ member_kickoff_starting' = (member_kickoff_starting \ {member_id})
    /\ member_kickoff_callback_pending' = (member_kickoff_callback_pending \ {member_id})
    /\ member_kickoff_started' = (member_kickoff_started \ {member_id})
    /\ member_kickoff_failed' = (member_kickoff_failed \ {member_id})
    /\ member_kickoff_cancelled' = (member_kickoff_cancelled \ {member_id})
    /\ member_kickoff_error' = MapRemove(member_kickoff_error, member_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubmitWorkRunningExternal(agent_runtime_id, fence_token, work_id, origin) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (origin = "External")
    /\ (agent_runtime_id \in externally_addressable_runtime_ids)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubmitWorkRunningInternal(agent_runtime_id, fence_token, work_id, origin) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (origin = "Internal")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireMember(agent_runtime_id, fence_token, session_id) ==
    /\ phase = "Running"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ObserveRuntimeRetired(agent_runtime_id, fence_token) ==
    /\ phase = "Running"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = (live_runtime_ids \ {agent_runtime_id})
    /\ externally_addressable_runtime_ids' = (externally_addressable_runtime_ids \ {agent_runtime_id})
    /\ runtime_fence_tokens' = MapRemove(runtime_fence_tokens, agent_runtime_id)
    /\ active_run_count' = 0
    /\ member_startup_binding_requested' = (member_startup_binding_requested \ {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ member_state_markers' = MapRemove(member_state_markers, agent_runtime_id)
    /\ UNCHANGED << run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResetMember(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, session_id) ==
    /\ phase = "Running" \/ phase = "Stopped"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = (live_runtime_ids \cup {agent_runtime_id})
    /\ externally_addressable_runtime_ids' = IF external_addressable THEN (externally_addressable_runtime_ids \cup {agent_runtime_id}) ELSE (externally_addressable_runtime_ids \ {agent_runtime_id})
    /\ runtime_fence_tokens' = MapSet(runtime_fence_tokens, agent_runtime_id, fence_token)
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ member_startup_binding_requested' = (member_startup_binding_requested \cup {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ identity_to_runtime' = MapSet(identity_to_runtime, agent_identity, agent_runtime_id)
    /\ UNCHANGED << run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RespawnMember(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, session_id) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = (live_runtime_ids \cup {agent_runtime_id})
    /\ externally_addressable_runtime_ids' = IF external_addressable THEN (externally_addressable_runtime_ids \cup {agent_runtime_id}) ELSE (externally_addressable_runtime_ids \ {agent_runtime_id})
    /\ runtime_fence_tokens' = MapSet(runtime_fence_tokens, agent_runtime_id, fence_token)
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ member_startup_binding_requested' = (member_startup_binding_requested \cup {agent_runtime_id})
    /\ member_startup_runtime_ready' = (member_startup_runtime_ready \ {agent_runtime_id})
    /\ member_startup_ready' = (member_startup_ready \ {agent_runtime_id})
    /\ identity_to_runtime' = MapSet(identity_to_runtime, agent_identity, agent_runtime_id)
    /\ UNCHANGED << run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, coordinator_bound, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


MarkCompleted ==
    /\ phase = "Running" \/ phase = "Stopped"
    /\ (active_run_count = 0)
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


DestroyMob(session_id) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ (pending_session_ingress_detach_runtime_ids = {})
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = {}
    /\ runtime_fence_tokens' = [x \in {} |-> None]
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ coordinator_bound' = FALSE
    /\ member_startup_binding_requested' = {}
    /\ member_startup_runtime_ready' = {}
    /\ member_startup_ready' = {}
    /\ member_state_markers' = [x \in {} |-> None]
    /\ pending_session_ingress_detach_runtime_ids' = {}
    /\ UNCHANGED << externally_addressable_runtime_ids, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, topology_epoch >>


ObserveRuntimeDestroyed(agent_runtime_id, fence_token) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed" \/ phase = "Destroyed"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = {}
    /\ runtime_fence_tokens' = [x \in {} |-> None]
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ coordinator_bound' = FALSE
    /\ member_state_markers' = [x \in {} |-> None]
    /\ UNCHANGED << externally_addressable_runtime_ids, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordOperatorActionProvenanceRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordOperatorActionProvenanceStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordOperatorActionProvenanceCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordOperatorActionProvenanceDestroyed ==
    /\ phase = "Destroyed"
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SetSpawnPolicyRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SetSpawnPolicyStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SetSpawnPolicyCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SetSpawnPolicyDestroyed ==
    /\ phase = "Destroyed"
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StopRunning ==
    /\ phase = "Running"
    /\ (active_run_count = 0)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResumeStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CompleteRunning ==
    /\ phase = "Running"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResetToRunning ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


WireMembersRunning(edge) ==
    /\ phase = "Running"
    /\ ((edge \in wiring_edges) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ wiring_edges' = (wiring_edges \cup {edge})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids >>


UnwireMembersRunning(edge) ==
    /\ phase = "Running"
    /\ ((edge \in wiring_edges) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ wiring_edges' = (wiring_edges \ {edge})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids >>


WireExternalPeerRunning(edge) ==
    /\ phase = "Running"
    /\ ((edge \in external_peer_edges) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ external_peer_edges' = (external_peer_edges \cup {edge})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids >>


UnwireExternalPeerRunning(edge) ==
    /\ phase = "Running"
    /\ ((edge \in external_peer_edges) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ external_peer_edges' = (external_peer_edges \ {edge})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids >>


BindMemberSessionRunning(agent_identity, session_id) ==
    /\ phase = "Running"
    /\ ((agent_identity \in DOMAIN identity_to_runtime) = TRUE)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_session_bindings' = MapSet(member_session_bindings, agent_identity, session_id)
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, pending_session_ingress_detach_runtime_ids >>


RotateMemberSessionRunning(agent_identity, old_session_id, new_session_id) ==
    /\ phase = "Running"
    /\ ((agent_identity \in DOMAIN identity_to_runtime) = TRUE)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ ((IF (agent_identity \in DOMAIN member_session_bindings) THEN Some((IF agent_identity \in DOMAIN member_session_bindings THEN member_session_bindings[agent_identity] ELSE "None")) ELSE None) = Some(old_session_id))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_session_bindings' = MapSet(member_session_bindings, agent_identity, new_session_id)
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, pending_session_ingress_detach_runtime_ids >>


ReleaseMemberSessionRunning(agent_identity, session_id) ==
    /\ phase = "Running"
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ ((IF (agent_identity \in DOMAIN member_session_bindings) THEN Some((IF agent_identity \in DOMAIN member_session_bindings THEN member_session_bindings[agent_identity] ELSE "None")) ELSE None) = Some(session_id))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_session_bindings' = MapRemove(member_session_bindings, agent_identity)
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, pending_session_ingress_detach_runtime_ids >>


TaskCreateRunning(task_id, task_payload) ==
    /\ phase = "Running"
    /\ ((task_id \in DOMAIN tasks) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ tasks' = MapSet(tasks, task_id, task_payload)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


TaskUpdateRunningPending(task_id, new_status) ==
    /\ phase = "Running"
    /\ (new_status = "Pending")
    /\ ((task_id \in DOMAIN tasks) = TRUE)
    /\ ((task_id \in completed_task_ids) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ in_progress_task_ids' = (in_progress_task_ids \ {task_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


TaskUpdateRunningInProgress(task_id, new_status) ==
    /\ phase = "Running"
    /\ (new_status = "InProgress")
    /\ ((task_id \in DOMAIN tasks) = TRUE)
    /\ ((task_id \in completed_task_ids) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ in_progress_task_ids' = (in_progress_task_ids \cup {task_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


TaskUpdateRunningCompleted(task_id, new_status) ==
    /\ phase = "Running"
    /\ (new_status = "Completed")
    /\ ((task_id \in DOMAIN tasks) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ in_progress_task_ids' = (in_progress_task_ids \ {task_id})
    /\ completed_task_ids' = (completed_task_ids \cup {task_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


TaskUpdateRunningCancelled(task_id, new_status) ==
    /\ phase = "Running"
    /\ (new_status = "Cancelled")
    /\ ((task_id \in DOMAIN tasks) = TRUE)
    /\ ((task_id \in completed_task_ids) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ in_progress_task_ids' = (in_progress_task_ids \ {task_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ForceCancelRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAgentEventsRunning ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAgentEventsStopped ==
    /\ phase = "Stopped"
    /\ (live_runtime_ids # {})
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAgentEventsCompleted ==
    /\ phase = "Completed"
    /\ (live_runtime_ids # {})
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAgentEventsDestroyed ==
    /\ phase = "Destroyed"
    /\ (live_runtime_ids # {})
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAllAgentEventsRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAllAgentEventsStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAllAgentEventsCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeAllAgentEventsDestroyed ==
    /\ phase = "Destroyed"
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeMobEventsRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeMobEventsStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeMobEventsCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SubscribeMobEventsDestroyed ==
    /\ phase = "Destroyed"
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ShutdownRunning ==
    /\ phase = "Running"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ShutdownStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ShutdownCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CancelFlowRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


InitializeOrchestratorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


BindCoordinatorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


UnbindCoordinatorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StageSpawnRunning(agent_identity, session_id) ==
    /\ phase = "Running"
    /\ ((agent_identity \in DOMAIN pending_spawn_sessions) = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_spawn_count' = (pending_spawn_count) + 1
    /\ pending_spawn_sessions' = MapSet(pending_spawn_sessions, agent_identity, session_id)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StopOrchestratorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StopOrchestratorStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StopOrchestratorCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResumeOrchestratorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResumeOrchestratorStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ResumeOrchestratorCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = TRUE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


DestroyOrchestratorRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


DestroyOrchestratorStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


DestroyOrchestratorCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Completed"
    /\ model_step_count' = model_step_count + 1
    /\ coordinator_bound' = FALSE
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


ForceCancelMemberRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


MemberPeerExposedRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


MemberTerminalizedRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


OperationPeerTrustedRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


PeerInputAdmittedRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


BeginCleanupStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


BeginCleanupCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


FinishCleanupStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


FinishCleanupCompleted ==
    /\ phase = "Completed"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RunFlowRunning ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CreateRunSeedRunning(run_id, step_ids, ordered_steps, step_has_conditions, step_dependencies, step_dependency_modes, step_branches, step_collection_policies, step_quorum_thresholds, escalation_threshold, max_step_retries, max_active_nodes, max_active_frames, max_frame_depth) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_status' = MapSet(run_status, run_id, "Pending")
    /\ run_ordered_steps' = MapSet(run_ordered_steps, run_id, ordered_steps)
    /\ run_tracked_steps' = MapSet(run_tracked_steps, run_id, step_ids)
    /\ run_step_status' = MapSet(run_step_status, run_id, [x \in {} |-> None])
    /\ run_output_recorded' = MapSet(run_output_recorded, run_id, [x \in {} |-> None])
    /\ run_step_condition_results' = MapSet(run_step_condition_results, run_id, [x \in {} |-> None])
    /\ run_step_has_conditions' = MapSet(run_step_has_conditions, run_id, step_has_conditions)
    /\ run_step_dependencies' = MapSet(run_step_dependencies, run_id, step_dependencies)
    /\ run_step_dependency_modes' = MapSet(run_step_dependency_modes, run_id, step_dependency_modes)
    /\ run_step_branches' = MapSet(run_step_branches, run_id, step_branches)
    /\ run_step_collection_policies' = MapSet(run_step_collection_policies, run_id, step_collection_policies)
    /\ run_step_quorum_thresholds' = MapSet(run_step_quorum_thresholds, run_id, step_quorum_thresholds)
    /\ run_step_target_counts' = MapSet(run_step_target_counts, run_id, [x \in {} |-> None])
    /\ run_step_target_success_counts' = MapSet(run_step_target_success_counts, run_id, [x \in {} |-> None])
    /\ run_step_target_terminal_failure_counts' = MapSet(run_step_target_terminal_failure_counts, run_id, [x \in {} |-> None])
    /\ run_target_retry_counts' = MapSet(run_target_retry_counts, run_id, [x \in {} |-> None])
    /\ run_escalation_threshold' = MapSet(run_escalation_threshold, run_id, escalation_threshold)
    /\ run_max_step_retries' = MapSet(run_max_step_retries, run_id, max_step_retries)
    /\ run_ready_frames' = MapSet(run_ready_frames, run_id, <<>>)
    /\ run_ready_frame_membership' = MapSet(run_ready_frame_membership, run_id, {})
    /\ run_pending_body_frame_loops' = MapSet(run_pending_body_frame_loops, run_id, <<>>)
    /\ run_pending_body_frame_loop_membership' = MapSet(run_pending_body_frame_loop_membership, run_id, {})
    /\ run_max_active_nodes' = MapSet(run_max_active_nodes, run_id, max_active_nodes)
    /\ run_max_active_frames' = MapSet(run_max_active_frames, run_id, max_active_frames)
    /\ run_max_frame_depth' = MapSet(run_max_frame_depth, run_id, max_frame_depth)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_step_status_flat, run_output_recorded_flat, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CreateFrameSeedRunning(run_id, frame_id, arg_frame_scope, loop_instance_id, iteration, tracked_nodes, ordered_nodes, node_kind, node_dependencies, node_dependency_modes, node_branches) ==
    /\ phase = "Running"
    /\ ((arg_frame_scope # "Body") \/ (loop_instance_id # None))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ frame_scope' = MapSet(frame_scope, frame_id, arg_frame_scope)
    /\ frame_phase' = MapSet(frame_phase, frame_id, "Running")
    /\ frame_run' = MapSet(frame_run, frame_id, run_id)
    /\ frame_parent_loop' = MapSet(frame_parent_loop, frame_id, loop_instance_id)
    /\ frame_iteration' = MapSet(frame_iteration, frame_id, iteration)
    /\ frame_tracked_nodes' = MapSet(frame_tracked_nodes, frame_id, tracked_nodes)
    /\ frame_ordered_nodes' = MapSet(frame_ordered_nodes, frame_id, ordered_nodes)
    /\ frame_node_kind' = MapSet(frame_node_kind, frame_id, node_kind)
    /\ frame_node_dependencies' = MapSet(frame_node_dependencies, frame_id, node_dependencies)
    /\ frame_node_dependency_modes' = MapSet(frame_node_dependency_modes, frame_id, node_dependency_modes)
    /\ frame_node_status' = MapSet(frame_node_status, frame_id, [x \in {} |-> None])
    /\ frame_ready_queue' = MapSet(frame_ready_queue, frame_id, <<>>)
    /\ frame_output_recorded' = MapSet(frame_output_recorded, frame_id, [x \in {} |-> None])
    /\ frame_node_condition_results' = MapSet(frame_node_condition_results, frame_id, [x \in {} |-> None])
    /\ frame_node_branches' = MapSet(frame_node_branches, frame_id, node_branches)
    /\ loop_stage' = IF (arg_frame_scope = "Body") THEN MapSet(loop_stage, (IF "value" \in DOMAIN loop_instance_id THEN loop_instance_id["value"] ELSE None), "BodyFrameActive") ELSE loop_stage
    /\ loop_active_body_frame' = IF (arg_frame_scope = "Body") THEN MapSet(loop_active_body_frame, (IF "value" \in DOMAIN loop_instance_id THEN loop_instance_id["value"] ELSE None), Some(frame_id)) ELSE loop_active_body_frame
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_node_step_ids, frame_node_loop_ids, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CreateLoopSeedRunning(loop_instance_id, parent_frame_id, parent_node_id, loop_id, depth, max_iterations) ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Running")
    /\ loop_parent_frame' = MapSet(loop_parent_frame, loop_instance_id, parent_frame_id)
    /\ loop_parent_node' = MapSet(loop_parent_node, loop_instance_id, parent_node_id)
    /\ loop_definition' = MapSet(loop_definition, loop_instance_id, loop_id)
    /\ loop_depth' = MapSet(loop_depth, loop_instance_id, depth)
    /\ loop_stage' = MapSet(loop_stage, loop_instance_id, "AwaitingBodyFrame")
    /\ loop_current_iteration' = MapSet(loop_current_iteration, loop_instance_id, 0)
    /\ loop_last_completed_iteration' = MapSet(loop_last_completed_iteration, loop_instance_id, 0)
    /\ loop_max_iterations' = MapSet(loop_max_iterations, loop_instance_id, max_iterations)
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordLoopBodyFrameCompletedRunning(loop_instance_id, iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("BodyFrameActive"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) = Some(iteration))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_stage' = MapSet(loop_stage, loop_instance_id, "AwaitingUntilEvaluation")
    /\ loop_current_iteration' = MapSet(loop_current_iteration, loop_instance_id, (iteration + 1))
    /\ loop_last_completed_iteration' = MapSet(loop_last_completed_iteration, loop_instance_id, iteration)
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordLoopUntilConditionMetRunning(loop_instance_id, iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("AwaitingUntilEvaluation"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_last_completed_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_last_completed_iteration THEN loop_last_completed_iteration[loop_instance_id] ELSE 0)) ELSE None) = Some(iteration))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Completed")
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordLoopUntilConditionFailedRunning(loop_instance_id, iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("AwaitingUntilEvaluation"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_last_completed_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_last_completed_iteration THEN loop_last_completed_iteration[loop_instance_id] ELSE 0)) ELSE None) = Some(iteration))
    /\ ((IF "value" \in DOMAIN (IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) THEN (IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None)["value"] ELSE None) < (IF "value" \in DOMAIN (IF (loop_instance_id \in DOMAIN loop_max_iterations) THEN Some((IF loop_instance_id \in DOMAIN loop_max_iterations THEN loop_max_iterations[loop_instance_id] ELSE 0)) ELSE None) THEN (IF (loop_instance_id \in DOMAIN loop_max_iterations) THEN Some((IF loop_instance_id \in DOMAIN loop_max_iterations THEN loop_max_iterations[loop_instance_id] ELSE 0)) ELSE None)["value"] ELSE None))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_stage' = MapSet(loop_stage, loop_instance_id, "AwaitingBodyFrame")
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RecordLoopUntilConditionFailedExhausted(loop_instance_id, iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("AwaitingUntilEvaluation"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_last_completed_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_last_completed_iteration THEN loop_last_completed_iteration[loop_instance_id] ELSE 0)) ELSE None) = Some(iteration))
    /\ ((IF "value" \in DOMAIN (IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) THEN (IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None)["value"] ELSE None) >= (IF "value" \in DOMAIN (IF (loop_instance_id \in DOMAIN loop_max_iterations) THEN Some((IF loop_instance_id \in DOMAIN loop_max_iterations THEN loop_max_iterations[loop_instance_id] ELSE 0)) ELSE None) THEN (IF (loop_instance_id \in DOMAIN loop_max_iterations) THEN Some((IF loop_instance_id \in DOMAIN loop_max_iterations THEN loop_max_iterations[loop_instance_id] ELSE 0)) ELSE None)["value"] ELSE None))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Exhausted")
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandStartRun(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ (command = "StartRun")
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Pending"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_status' = MapSet(run_status, run_id, "Running")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandActiveRun(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ FALSE
    /\ ((command = "CompleteStep") \/ (command = "RecordStepOutput") \/ (command = "ConditionPassed") \/ (command = "ConditionRejected") \/ (command = "FailStep") \/ (command = "SkipStep") \/ (command = "ProjectFrameStepStatus") \/ (command = "RegisterTargets") \/ (command = "RecordTargetSuccess") \/ (command = "RecordTargetTerminalFailure") \/ (command = "RecordTargetCanceled") \/ (command = "RecordTargetFailure") \/ (command = "RegisterReadyFrame") \/ (command = "PumpNodeScheduler") \/ (command = "RegisterPendingBodyFrame") \/ (command = "PumpFrameScheduler") \/ (command = "NodeExecutionReleased") \/ (command = "FrameTerminated"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandDispatchStep(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "DispatchStep")
    /\ (step_id # None)
    /\ (run_step_key # None)
    /\ (step_status = Some("Dispatched"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_step_status_flat' = MapSet(run_step_status_flat, (IF "value" \in DOMAIN run_step_key THEN run_step_key["value"] ELSE None), "Dispatched")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandCancelStep(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "CancelStep")
    /\ (step_id # None)
    /\ (run_step_key # None)
    /\ (step_status = Some("Canceled"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_step_status_flat' = MapSet(run_step_status_flat, (IF "value" \in DOMAIN run_step_key THEN run_step_key["value"] ELSE None), "Canceled")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandTerminalCompleted(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "TerminalizeCompleted")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_status' = MapSet(run_status, run_id, "Completed")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandTerminalFailed(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "TerminalizeFailed")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_status' = MapSet(run_status, run_id, "Failed")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowRunReducerCommandTerminalCanceled(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((run_id \in DOMAIN run_status) = TRUE)
    /\ ((IF (run_id \in DOMAIN run_status) THEN Some((IF run_id \in DOMAIN run_status THEN run_status[run_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "TerminalizeCanceled")
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ run_status' = MapSet(run_status, run_id, "Canceled")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowFrameReducerCommandActiveFrame(frame_id, command, node_id, node_status, ready_queue, terminal_status) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((frame_id \in DOMAIN frame_phase) = TRUE)
    /\ ((IF (frame_id \in DOMAIN frame_phase) THEN Some((IF frame_id \in DOMAIN frame_phase THEN frame_phase[frame_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ FALSE
    /\ (terminal_status = None)
    /\ ((command = "AdmitNextReadyNode") \/ (command = "CompleteNode") \/ (command = "RecordNodeOutput") \/ (command = "FailNode") \/ (command = "SkipNode") \/ (command = "CancelNode"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeFlowFrameReducerCommandSealFrame(frame_id, command, node_id, node_status, ready_queue, terminal_status) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((frame_id \in DOMAIN frame_phase) = TRUE)
    /\ ((IF (frame_id \in DOMAIN frame_phase) THEN Some((IF frame_id \in DOMAIN frame_phase THEN frame_phase[frame_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "SealFrame")
    /\ ((terminal_status = Some("Completed")) \/ (terminal_status = Some("Failed")) \/ (terminal_status = Some("Canceled")))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ frame_phase' = MapSet(frame_phase, frame_id, (IF "value" \in DOMAIN terminal_status THEN terminal_status["value"] ELSE None))
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandBodyFrameStarted(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "BodyFrameStarted")
    /\ FALSE
    /\ (body_frame_iteration = None)
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("BodyFrameActive"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandBodyFrameCompleted(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("BodyFrameActive"))
    /\ (command = "BodyFrameCompleted")
    /\ FALSE
    /\ (body_frame_iteration # None)
    /\ ((IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) = body_frame_iteration)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandBodyFrameFailed(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("BodyFrameActive"))
    /\ (command = "BodyFrameFailed")
    /\ (body_frame_iteration # None)
    /\ ((IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) = body_frame_iteration)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Failed")
    /\ loop_last_completed_iteration' = MapSet(loop_last_completed_iteration, loop_instance_id, (IF "value" \in DOMAIN body_frame_iteration THEN body_frame_iteration["value"] ELSE None))
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandBodyFrameCanceled(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("BodyFrameActive"))
    /\ (command = "BodyFrameCanceled")
    /\ (body_frame_iteration # None)
    /\ ((IF (loop_instance_id \in DOMAIN loop_current_iteration) THEN Some((IF loop_instance_id \in DOMAIN loop_current_iteration THEN loop_current_iteration[loop_instance_id] ELSE 0)) ELSE None) = body_frame_iteration)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Canceled")
    /\ loop_last_completed_iteration' = MapSet(loop_last_completed_iteration, loop_instance_id, (IF "value" \in DOMAIN body_frame_iteration THEN body_frame_iteration["value"] ELSE None))
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandUntilFeedback(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ ((IF (loop_instance_id \in DOMAIN loop_stage) THEN Some((IF loop_instance_id \in DOMAIN loop_stage THEN loop_stage[loop_instance_id] ELSE "None")) ELSE None) = Some("AwaitingUntilEvaluation"))
    /\ FALSE
    /\ (body_frame_iteration = None)
    /\ ((command = "UntilConditionMet") \/ (command = "UntilConditionFailed"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


AuthorizeLoopIterationReducerCommandCancelLoop(loop_instance_id, command, body_frame_id, body_frame_iteration) ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ ((loop_instance_id \in DOMAIN loop_phase) = TRUE)
    /\ ((IF (loop_instance_id \in DOMAIN loop_phase) THEN Some((IF loop_instance_id \in DOMAIN loop_phase THEN loop_phase[loop_instance_id] ELSE "None")) ELSE None) = Some("Running"))
    /\ (command = "CancelLoop")
    /\ (body_frame_iteration = None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ loop_phase' = MapSet(loop_phase, loop_instance_id, "Canceled")
    /\ loop_active_body_frame' = MapSet(loop_active_body_frame, loop_instance_id, None)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StartFlowRunning ==
    /\ phase = "Running"
    /\ (coordinator_bound = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CreateRunRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


StartRunRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CompleteFlowRunning ==
    /\ phase = "Running" \/ phase = "Completed"
    /\ (active_run_count > 0)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) - 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CompleteFlowRunningZero ==
    /\ phase = "Running" \/ phase = "Completed"
    /\ (active_run_count = 0)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


FinishRunRunning ==
    /\ phase = "Running" \/ phase = "Stopped"
    /\ (active_run_count > 0)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = (active_run_count) - 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


FinishRunRunningZero ==
    /\ phase = "Running" \/ phase = "Stopped"
    /\ (active_run_count = 0)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireRunningReleasing(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ (releasing # None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ member_session_bindings' = MapRemove(member_session_bindings, agent_identity)
    /\ pending_session_ingress_detach_runtime_ids' = (pending_session_ingress_detach_runtime_ids \cup {agent_runtime_id})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids >>


RetireRunningPreservingBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ (releasing = None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireRunningNoBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = FALSE)
    /\ (releasing = None)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireStoppedReleasing(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Stopped"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ (releasing # None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ member_session_bindings' = MapRemove(member_session_bindings, agent_identity)
    /\ pending_session_ingress_detach_runtime_ids' = (pending_session_ingress_detach_runtime_ids \cup {agent_runtime_id})
    /\ topology_epoch' = (topology_epoch) + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids >>


SessionIngressDetachedForMobDestroyRunning(mob_id, agent_runtime_id) ==
    /\ phase = "Running"
    /\ ((agent_runtime_id \in pending_session_ingress_detach_runtime_ids) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_session_ingress_detach_runtime_ids' = (pending_session_ingress_detach_runtime_ids \ {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, topology_epoch >>


SessionIngressDetachedForMobDestroyStopped(mob_id, agent_runtime_id) ==
    /\ phase = "Stopped"
    /\ ((agent_runtime_id \in pending_session_ingress_detach_runtime_ids) = TRUE)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ pending_session_ingress_detach_runtime_ids' = (pending_session_ingress_detach_runtime_ids \ {agent_runtime_id})
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, topology_epoch >>


SessionIngressDetachFailedForMobDestroyRunning(mob_id, agent_runtime_id, reason) ==
    /\ phase = "Running"
    /\ ((agent_runtime_id \in pending_session_ingress_detach_runtime_ids) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


SessionIngressDetachFailedForMobDestroyStopped(mob_id, agent_runtime_id, reason) ==
    /\ phase = "Stopped"
    /\ ((agent_runtime_id \in pending_session_ingress_detach_runtime_ids) = TRUE)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireStoppedPreservingBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Stopped"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = TRUE)
    /\ (releasing = None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireStoppedNoBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id) ==
    /\ phase = "Stopped"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ ((agent_identity \in DOMAIN member_session_bindings) = FALSE)
    /\ (releasing = None)
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ member_state_markers' = MapSet(member_state_markers, agent_runtime_id, "Retiring")
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireAllRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = {}
    /\ runtime_fence_tokens' = [x \in {} |-> None]
    /\ UNCHANGED << externally_addressable_runtime_ids, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


RetireAllStopped ==
    /\ phase = "Stopped"
    /\ phase' = "Stopped"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = {}
    /\ runtime_fence_tokens' = [x \in {} |-> None]
    /\ UNCHANGED << externally_addressable_runtime_ids, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CompleteSpawnRunning(agent_identity) ==
    /\ phase = "Running" \/ phase = "Stopped"
    /\ (pending_spawn_count > 0)
    /\ ((agent_identity \in DOMAIN pending_spawn_sessions) = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ pending_spawn_count' = (pending_spawn_count) - 1
    /\ pending_spawn_sessions' = MapRemove(pending_spawn_sessions, agent_identity)
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


DestroyFromAny ==
    /\ phase = "Running" \/ phase = "Stopped" \/ phase = "Completed"
    /\ (pending_session_ingress_detach_runtime_ids = {})
    /\ phase' = "Destroyed"
    /\ model_step_count' = model_step_count + 1
    /\ live_runtime_ids' = {}
    /\ runtime_fence_tokens' = [x \in {} |-> None]
    /\ active_run_count' = 0
    /\ pending_spawn_count' = 0
    /\ pending_spawn_sessions' = [x \in {} |-> None]
    /\ coordinator_bound' = FALSE
    /\ pending_session_ingress_detach_runtime_ids' = {}
    /\ UNCHANGED << externally_addressable_runtime_ids, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, topology_epoch >>


RespawnRunning(agent_runtime_id) ==
    /\ phase = "Running"
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (coordinator_bound = TRUE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, active_run_count, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


CancelAllWorkRunning(agent_runtime_id, fence_token) ==
    /\ phase = "Running"
    /\ (live_runtime_ids # {})
    /\ (agent_runtime_id \in live_runtime_ids)
    /\ (fence_token = fence_token)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ active_run_count' = 0
    /\ UNCHANGED << live_runtime_ids, externally_addressable_runtime_ids, runtime_fence_tokens, run_status, run_ordered_steps, run_tracked_steps, run_step_status, run_step_status_flat, run_output_recorded, run_step_condition_results, run_step_has_conditions, run_step_dependencies, run_step_dependency_modes, run_step_branches, run_step_collection_policies, run_step_quorum_thresholds, run_step_target_counts, run_step_target_success_counts, run_step_target_terminal_failure_counts, run_output_recorded_flat, run_target_retry_counts, run_escalation_threshold, run_max_step_retries, run_ready_frames, run_ready_frame_membership, run_pending_body_frame_loops, run_pending_body_frame_loop_membership, run_max_active_nodes, run_max_active_frames, run_max_frame_depth, frame_scope, frame_phase, frame_run, frame_parent_loop, frame_iteration, frame_tracked_nodes, frame_ordered_nodes, frame_node_kind, frame_node_dependencies, frame_node_dependency_modes, frame_node_step_ids, frame_node_loop_ids, frame_node_status, frame_ready_queue, frame_output_recorded, frame_node_condition_results, frame_node_branches, loop_phase, loop_parent_frame, loop_parent_node, loop_definition, loop_depth, loop_stage, loop_current_iteration, loop_last_completed_iteration, loop_max_iterations, loop_active_body_frame, pending_spawn_count, pending_spawn_sessions, coordinator_bound, member_startup_binding_requested, member_startup_runtime_ready, member_startup_ready, member_kickoff_pending, member_kickoff_starting, member_kickoff_callback_pending, member_kickoff_started, member_kickoff_failed, member_kickoff_cancelled, member_kickoff_error, member_state_markers, wiring_edges, external_peer_edges, identity_to_runtime, tasks, in_progress_task_ids, completed_task_ids, member_session_bindings, pending_session_ingress_detach_runtime_ids, topology_epoch >>


Next ==
    \/ \E agent_identity \in AgentIdentityValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : \E external_addressable \in BOOLEAN : \E bridge_session_id \in SessionIdValues : \E replacing \in OptionSessionIdValues : SpawnRunningFresh(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, bridge_session_id, replacing)
    \/ \E agent_identity \in AgentIdentityValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : \E external_addressable \in BOOLEAN : \E bridge_session_id \in SessionIdValues : \E replacing \in OptionSessionIdValues : SpawnRunningReplacing(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, bridge_session_id, replacing)
    \/ \E agent_identity \in AgentIdentityValues : EnsureMemberRunningExisting(agent_identity)
    \/ \E agent_identity \in AgentIdentityValues : EnsureMemberRunningMissing(agent_identity)
    \/ \E desired \in SetOfAgentIdentityValues : \E retire_stale \in BOOLEAN : ReconcileRunning(desired, retire_stale)
    \/ \E desired \in SetOfAgentIdentityValues : \E retire_stale \in BOOLEAN : ReconcileStopped(desired, retire_stale)
    \/ \E desired \in SetOfAgentIdentityValues : \E retire_stale \in BOOLEAN : ReconcileCompleted(desired, retire_stale)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : ObserveRuntimeReady(agent_runtime_id, fence_token)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : StartupMarkReadyRunning(agent_runtime_id, fence_token)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : StartupMarkReadyStopped(agent_runtime_id, fence_token)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : StartupMarkReadyCompleted(agent_runtime_id, fence_token)
    \/ \E member_id \in StringValues : KickoffMarkPendingRunning(member_id)
    \/ \E member_id \in StringValues : KickoffMarkPendingStopped(member_id)
    \/ \E member_id \in StringValues : KickoffMarkPendingCompleted(member_id)
    \/ \E member_id \in StringValues : KickoffMarkStartingRunning(member_id)
    \/ \E member_id \in StringValues : KickoffMarkStartingStopped(member_id)
    \/ \E member_id \in StringValues : KickoffMarkStartingCompleted(member_id)
    \/ \E member_id \in StringValues : KickoffResolveStartedRunning(member_id)
    \/ \E member_id \in StringValues : KickoffResolveStartedStopped(member_id)
    \/ \E member_id \in StringValues : KickoffResolveStartedCompleted(member_id)
    \/ \E member_id \in StringValues : KickoffResolveCallbackPendingRunning(member_id)
    \/ \E member_id \in StringValues : KickoffResolveCallbackPendingStopped(member_id)
    \/ \E member_id \in StringValues : KickoffResolveCallbackPendingCompleted(member_id)
    \/ \E member_id \in StringValues : \E error \in StringValues : KickoffResolveFailedFromStartingRunning(member_id, error)
    \/ \E member_id \in StringValues : \E error \in StringValues : KickoffResolveFailedFromStartingStopped(member_id, error)
    \/ \E member_id \in StringValues : \E error \in StringValues : KickoffResolveFailedFromStartingCompleted(member_id, error)
    \/ \E member_id \in StringValues : KickoffResolveCancelledRunning(member_id)
    \/ \E member_id \in StringValues : KickoffResolveCancelledStopped(member_id)
    \/ \E member_id \in StringValues : KickoffResolveCancelledCompleted(member_id)
    \/ \E member_id \in StringValues : KickoffCancelRequestedRunning(member_id)
    \/ \E member_id \in StringValues : KickoffCancelRequestedStopped(member_id)
    \/ \E member_id \in StringValues : KickoffCancelRequestedCompleted(member_id)
    \/ \E member_id \in StringValues : KickoffClearRunning(member_id)
    \/ \E member_id \in StringValues : KickoffClearStopped(member_id)
    \/ \E member_id \in StringValues : KickoffClearCompleted(member_id)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E work_id \in WorkIdValues : \E origin \in WorkOriginValues : SubmitWorkRunningExternal(agent_runtime_id, fence_token, work_id, origin)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E work_id \in WorkIdValues : \E origin \in WorkOriginValues : SubmitWorkRunningInternal(agent_runtime_id, fence_token, work_id, origin)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E session_id \in SessionIdValues : RetireMember(agent_runtime_id, fence_token, session_id)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : ObserveRuntimeRetired(agent_runtime_id, fence_token)
    \/ \E agent_identity \in AgentIdentityValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : \E external_addressable \in BOOLEAN : \E session_id \in SessionIdValues : ResetMember(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, session_id)
    \/ \E agent_identity \in AgentIdentityValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : \E generation \in GenerationValues : \E external_addressable \in BOOLEAN : \E session_id \in SessionIdValues : RespawnMember(agent_identity, agent_runtime_id, fence_token, generation, external_addressable, session_id)
    \/ MarkCompleted
    \/ \E session_id \in SessionIdValues : DestroyMob(session_id)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : ObserveRuntimeDestroyed(agent_runtime_id, fence_token)
    \/ RecordOperatorActionProvenanceRunning
    \/ RecordOperatorActionProvenanceStopped
    \/ RecordOperatorActionProvenanceCompleted
    \/ RecordOperatorActionProvenanceDestroyed
    \/ SetSpawnPolicyRunning
    \/ SetSpawnPolicyStopped
    \/ SetSpawnPolicyCompleted
    \/ SetSpawnPolicyDestroyed
    \/ StopRunning
    \/ ResumeStopped
    \/ CompleteRunning
    \/ ResetToRunning
    \/ \E edge \in WiringEdgeValues : WireMembersRunning(edge)
    \/ \E edge \in WiringEdgeValues : UnwireMembersRunning(edge)
    \/ \E edge \in ExternalPeerEdgeValues : WireExternalPeerRunning(edge)
    \/ \E edge \in ExternalPeerEdgeValues : UnwireExternalPeerRunning(edge)
    \/ \E agent_identity \in AgentIdentityValues : \E session_id \in SessionIdValues : BindMemberSessionRunning(agent_identity, session_id)
    \/ \E agent_identity \in AgentIdentityValues : \E old_session_id \in SessionIdValues : \E new_session_id \in SessionIdValues : RotateMemberSessionRunning(agent_identity, old_session_id, new_session_id)
    \/ \E agent_identity \in AgentIdentityValues : \E session_id \in SessionIdValues : ReleaseMemberSessionRunning(agent_identity, session_id)
    \/ \E task_id \in TaskIdValues : \E task_payload \in MobTaskValues : TaskCreateRunning(task_id, task_payload)
    \/ \E task_id \in TaskIdValues : \E new_status \in TaskStatusValues : TaskUpdateRunningPending(task_id, new_status)
    \/ \E task_id \in TaskIdValues : \E new_status \in TaskStatusValues : TaskUpdateRunningInProgress(task_id, new_status)
    \/ \E task_id \in TaskIdValues : \E new_status \in TaskStatusValues : TaskUpdateRunningCompleted(task_id, new_status)
    \/ \E task_id \in TaskIdValues : \E new_status \in TaskStatusValues : TaskUpdateRunningCancelled(task_id, new_status)
    \/ ForceCancelRunning
    \/ SubscribeAgentEventsRunning
    \/ SubscribeAgentEventsStopped
    \/ SubscribeAgentEventsCompleted
    \/ SubscribeAgentEventsDestroyed
    \/ SubscribeAllAgentEventsRunning
    \/ SubscribeAllAgentEventsStopped
    \/ SubscribeAllAgentEventsCompleted
    \/ SubscribeAllAgentEventsDestroyed
    \/ SubscribeMobEventsRunning
    \/ SubscribeMobEventsStopped
    \/ SubscribeMobEventsCompleted
    \/ SubscribeMobEventsDestroyed
    \/ ShutdownRunning
    \/ ShutdownStopped
    \/ ShutdownCompleted
    \/ CancelFlowRunning
    \/ InitializeOrchestratorRunning
    \/ BindCoordinatorRunning
    \/ UnbindCoordinatorRunning
    \/ \E agent_identity \in AgentIdentityValues : \E session_id \in SessionIdValues : StageSpawnRunning(agent_identity, session_id)
    \/ StopOrchestratorRunning
    \/ StopOrchestratorStopped
    \/ StopOrchestratorCompleted
    \/ ResumeOrchestratorRunning
    \/ ResumeOrchestratorStopped
    \/ ResumeOrchestratorCompleted
    \/ DestroyOrchestratorRunning
    \/ DestroyOrchestratorStopped
    \/ DestroyOrchestratorCompleted
    \/ ForceCancelMemberRunning
    \/ MemberPeerExposedRunning
    \/ MemberTerminalizedRunning
    \/ OperationPeerTrustedRunning
    \/ PeerInputAdmittedRunning
    \/ BeginCleanupStopped
    \/ BeginCleanupCompleted
    \/ FinishCleanupStopped
    \/ FinishCleanupCompleted
    \/ RunFlowRunning
    \/ \E run_id \in RunIdValues : \E step_ids \in SetOfStepIdValues : \E ordered_steps \in SeqOfStepIdValues : \E step_has_conditions \in MapStepIdBoolValues : \E step_dependencies \in MapStepIdSeqStepIdValues : \E step_dependency_modes \in MapStepIdDependencyModeValues : \E step_branches \in MapStepIdOptionBranchIdValues : \E step_collection_policies \in MapStepIdCollectionPolicyKindValues : \E step_quorum_thresholds \in MapStepIdU32Values : \E escalation_threshold \in 0..2 : \E max_step_retries \in 0..2 : \E max_active_nodes \in 0..2 : \E max_active_frames \in 0..2 : \E max_frame_depth \in 0..2 : CreateRunSeedRunning(run_id, step_ids, ordered_steps, step_has_conditions, step_dependencies, step_dependency_modes, step_branches, step_collection_policies, step_quorum_thresholds, escalation_threshold, max_step_retries, max_active_nodes, max_active_frames, max_frame_depth)
    \/ \E run_id \in RunIdValues : \E frame_id \in FrameIdValues : \E arg_frame_scope \in FrameScopeValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E iteration \in 0..2 : \E tracked_nodes \in SetOfFlowNodeIdValues : \E ordered_nodes \in SeqOfFlowNodeIdValues : \E node_kind \in MapFlowNodeIdFlowNodeKindValues : \E node_dependencies \in MapFlowNodeIdSeqFlowNodeIdValues : \E node_dependency_modes \in MapFlowNodeIdDependencyModeValues : \E node_branches \in MapFlowNodeIdOptionBranchIdValues : CreateFrameSeedRunning(run_id, frame_id, arg_frame_scope, loop_instance_id, iteration, tracked_nodes, ordered_nodes, node_kind, node_dependencies, node_dependency_modes, node_branches)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E parent_frame_id \in FrameIdValues : \E parent_node_id \in FlowNodeIdValues : \E loop_id \in LoopIdValues : \E depth \in 0..2 : \E max_iterations \in 0..2 : CreateLoopSeedRunning(loop_instance_id, parent_frame_id, parent_node_id, loop_id, depth, max_iterations)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E iteration \in 0..2 : RecordLoopBodyFrameCompletedRunning(loop_instance_id, iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E iteration \in 0..2 : RecordLoopUntilConditionMetRunning(loop_instance_id, iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E iteration \in 0..2 : RecordLoopUntilConditionFailedRunning(loop_instance_id, iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E iteration \in 0..2 : RecordLoopUntilConditionFailedExhausted(loop_instance_id, iteration)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandStartRun(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandActiveRun(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandDispatchStep(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandCancelStep(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandTerminalCompleted(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandTerminalFailed(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E run_id \in RunIdValues : \E command \in FlowRunReducerCommandKindValues : \E step_id \in OptionStepIdValues : \E run_step_key \in OptionRunStepKeyValues : \E step_status \in OptionStepRunStatusValues : \E target_count \in OptionU32Values : \E frame_id \in OptionFrameIdValues : \E loop_instance_id \in OptionLoopInstanceIdValues : \E retry_key \in OptionStringValues : AuthorizeFlowRunReducerCommandTerminalCanceled(run_id, command, step_id, run_step_key, step_status, target_count, frame_id, loop_instance_id, retry_key)
    \/ \E frame_id \in FrameIdValues : \E command \in FlowFrameReducerCommandKindValues : \E node_id \in OptionFlowNodeIdValues : \E node_status \in OptionNodeRunStatusValues : \E ready_queue \in OptionSeqFlowNodeIdValues : \E terminal_status \in OptionFrameStatusValues : AuthorizeFlowFrameReducerCommandActiveFrame(frame_id, command, node_id, node_status, ready_queue, terminal_status)
    \/ \E frame_id \in FrameIdValues : \E command \in FlowFrameReducerCommandKindValues : \E node_id \in OptionFlowNodeIdValues : \E node_status \in OptionNodeRunStatusValues : \E ready_queue \in OptionSeqFlowNodeIdValues : \E terminal_status \in OptionFrameStatusValues : AuthorizeFlowFrameReducerCommandSealFrame(frame_id, command, node_id, node_status, ready_queue, terminal_status)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandBodyFrameStarted(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandBodyFrameCompleted(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandBodyFrameFailed(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandBodyFrameCanceled(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandUntilFeedback(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ \E loop_instance_id \in LoopInstanceIdValues : \E command \in LoopIterationReducerCommandKindValues : \E body_frame_id \in OptionFrameIdValues : \E body_frame_iteration \in OptionU64Values : AuthorizeLoopIterationReducerCommandCancelLoop(loop_instance_id, command, body_frame_id, body_frame_iteration)
    \/ StartFlowRunning
    \/ CreateRunRunning
    \/ StartRunRunning
    \/ CompleteFlowRunning
    \/ CompleteFlowRunningZero
    \/ FinishRunRunning
    \/ FinishRunRunningZero
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireRunningReleasing(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireRunningPreservingBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireRunningNoBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireStoppedReleasing(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : SessionIngressDetachedForMobDestroyRunning(mob_id, agent_runtime_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : SessionIngressDetachedForMobDestroyStopped(mob_id, agent_runtime_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E reason \in StringValues : SessionIngressDetachFailedForMobDestroyRunning(mob_id, agent_runtime_id, reason)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E reason \in StringValues : SessionIngressDetachFailedForMobDestroyStopped(mob_id, agent_runtime_id, reason)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireStoppedPreservingBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ \E mob_id \in MobIdValues : \E agent_runtime_id \in AgentRuntimeIdValues : \E agent_identity \in AgentIdentityValues : \E releasing \in OptionSessionIdValues : \E session_id \in SessionIdValues : RetireStoppedNoBinding(mob_id, agent_runtime_id, agent_identity, releasing, session_id)
    \/ RetireAllRunning
    \/ RetireAllStopped
    \/ \E agent_identity \in AgentIdentityValues : CompleteSpawnRunning(agent_identity)
    \/ DestroyFromAny
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : RespawnRunning(agent_runtime_id)
    \/ \E agent_runtime_id \in AgentRuntimeIdValues : \E fence_token \in FenceTokenValues : CancelAllWorkRunning(agent_runtime_id, fence_token)
    \/ TerminalStutter

bindings_require_known_identity == (\A id \in DOMAIN member_session_bindings : (id \in DOMAIN identity_to_runtime))

CiStateConstraint == /\ model_step_count <= 1 /\ Cardinality(live_runtime_ids) <= 1 /\ Cardinality(externally_addressable_runtime_ids) <= 1 /\ Cardinality(DOMAIN runtime_fence_tokens) <= 1 /\ Cardinality(DOMAIN run_status) <= 1 /\ Cardinality(DOMAIN run_ordered_steps) <= 1 /\ Cardinality(DOMAIN run_tracked_steps) <= 1 /\ Cardinality(DOMAIN run_step_status) <= 1 /\ Cardinality(DOMAIN run_step_status_flat) <= 1 /\ Cardinality(DOMAIN run_output_recorded) <= 1 /\ Cardinality(DOMAIN run_step_condition_results) <= 1 /\ Cardinality(DOMAIN run_step_has_conditions) <= 1 /\ Cardinality(DOMAIN run_step_dependencies) <= 1 /\ Cardinality(DOMAIN run_step_dependency_modes) <= 1 /\ Cardinality(DOMAIN run_step_branches) <= 1 /\ Cardinality(DOMAIN run_step_collection_policies) <= 1 /\ Cardinality(DOMAIN run_step_quorum_thresholds) <= 1 /\ Cardinality(DOMAIN run_step_target_counts) <= 1 /\ Cardinality(DOMAIN run_step_target_success_counts) <= 1 /\ Cardinality(DOMAIN run_step_target_terminal_failure_counts) <= 1 /\ Cardinality(DOMAIN run_output_recorded_flat) <= 1 /\ Cardinality(DOMAIN run_target_retry_counts) <= 1 /\ Cardinality(DOMAIN run_escalation_threshold) <= 1 /\ Cardinality(DOMAIN run_max_step_retries) <= 1 /\ Cardinality(DOMAIN run_ready_frames) <= 1 /\ Cardinality(DOMAIN run_ready_frame_membership) <= 1 /\ Cardinality(DOMAIN run_pending_body_frame_loops) <= 1 /\ Cardinality(DOMAIN run_pending_body_frame_loop_membership) <= 1 /\ Cardinality(DOMAIN run_max_active_nodes) <= 1 /\ Cardinality(DOMAIN run_max_active_frames) <= 1 /\ Cardinality(DOMAIN run_max_frame_depth) <= 1 /\ Cardinality(DOMAIN frame_scope) <= 1 /\ Cardinality(DOMAIN frame_phase) <= 1 /\ Cardinality(DOMAIN frame_run) <= 1 /\ Cardinality(DOMAIN frame_parent_loop) <= 1 /\ Cardinality(DOMAIN frame_iteration) <= 1 /\ Cardinality(DOMAIN frame_tracked_nodes) <= 1 /\ Cardinality(DOMAIN frame_ordered_nodes) <= 1 /\ Cardinality(DOMAIN frame_node_kind) <= 1 /\ Cardinality(DOMAIN frame_node_dependencies) <= 1 /\ Cardinality(DOMAIN frame_node_dependency_modes) <= 1 /\ Cardinality(DOMAIN frame_node_step_ids) <= 1 /\ Cardinality(DOMAIN frame_node_loop_ids) <= 1 /\ Cardinality(DOMAIN frame_node_status) <= 1 /\ Cardinality(DOMAIN frame_ready_queue) <= 1 /\ Cardinality(DOMAIN frame_output_recorded) <= 1 /\ Cardinality(DOMAIN frame_node_condition_results) <= 1 /\ Cardinality(DOMAIN frame_node_branches) <= 1 /\ Cardinality(DOMAIN loop_phase) <= 1 /\ Cardinality(DOMAIN loop_parent_frame) <= 1 /\ Cardinality(DOMAIN loop_parent_node) <= 1 /\ Cardinality(DOMAIN loop_definition) <= 1 /\ Cardinality(DOMAIN loop_depth) <= 1 /\ Cardinality(DOMAIN loop_stage) <= 1 /\ Cardinality(DOMAIN loop_current_iteration) <= 1 /\ Cardinality(DOMAIN loop_last_completed_iteration) <= 1 /\ Cardinality(DOMAIN loop_max_iterations) <= 1 /\ Cardinality(DOMAIN loop_active_body_frame) <= 1 /\ Cardinality(DOMAIN pending_spawn_sessions) <= 1 /\ Cardinality(member_startup_binding_requested) <= 1 /\ Cardinality(member_startup_runtime_ready) <= 1 /\ Cardinality(member_startup_ready) <= 1 /\ Cardinality(member_kickoff_pending) <= 1 /\ Cardinality(member_kickoff_starting) <= 1 /\ Cardinality(member_kickoff_callback_pending) <= 1 /\ Cardinality(member_kickoff_started) <= 1 /\ Cardinality(member_kickoff_failed) <= 1 /\ Cardinality(member_kickoff_cancelled) <= 1 /\ Cardinality(DOMAIN member_kickoff_error) <= 1 /\ Cardinality(DOMAIN member_state_markers) <= 1 /\ Cardinality(wiring_edges) <= 1 /\ Cardinality(external_peer_edges) <= 1 /\ Cardinality(DOMAIN identity_to_runtime) <= 1 /\ Cardinality(DOMAIN tasks) <= 1 /\ Cardinality(in_progress_task_ids) <= 1 /\ Cardinality(completed_task_ids) <= 1 /\ Cardinality(DOMAIN member_session_bindings) <= 1 /\ Cardinality(pending_session_ingress_detach_runtime_ids) <= 1
DeepStateConstraint == /\ model_step_count <= 8 /\ Cardinality(live_runtime_ids) <= 2 /\ Cardinality(externally_addressable_runtime_ids) <= 2 /\ Cardinality(DOMAIN runtime_fence_tokens) <= 2 /\ Cardinality(DOMAIN run_status) <= 2 /\ Cardinality(DOMAIN run_ordered_steps) <= 2 /\ Cardinality(DOMAIN run_tracked_steps) <= 2 /\ Cardinality(DOMAIN run_step_status) <= 2 /\ Cardinality(DOMAIN run_step_status_flat) <= 2 /\ Cardinality(DOMAIN run_output_recorded) <= 2 /\ Cardinality(DOMAIN run_step_condition_results) <= 2 /\ Cardinality(DOMAIN run_step_has_conditions) <= 2 /\ Cardinality(DOMAIN run_step_dependencies) <= 2 /\ Cardinality(DOMAIN run_step_dependency_modes) <= 2 /\ Cardinality(DOMAIN run_step_branches) <= 2 /\ Cardinality(DOMAIN run_step_collection_policies) <= 2 /\ Cardinality(DOMAIN run_step_quorum_thresholds) <= 2 /\ Cardinality(DOMAIN run_step_target_counts) <= 2 /\ Cardinality(DOMAIN run_step_target_success_counts) <= 2 /\ Cardinality(DOMAIN run_step_target_terminal_failure_counts) <= 2 /\ Cardinality(DOMAIN run_output_recorded_flat) <= 2 /\ Cardinality(DOMAIN run_target_retry_counts) <= 2 /\ Cardinality(DOMAIN run_escalation_threshold) <= 2 /\ Cardinality(DOMAIN run_max_step_retries) <= 2 /\ Cardinality(DOMAIN run_ready_frames) <= 2 /\ Cardinality(DOMAIN run_ready_frame_membership) <= 2 /\ Cardinality(DOMAIN run_pending_body_frame_loops) <= 2 /\ Cardinality(DOMAIN run_pending_body_frame_loop_membership) <= 2 /\ Cardinality(DOMAIN run_max_active_nodes) <= 2 /\ Cardinality(DOMAIN run_max_active_frames) <= 2 /\ Cardinality(DOMAIN run_max_frame_depth) <= 2 /\ Cardinality(DOMAIN frame_scope) <= 2 /\ Cardinality(DOMAIN frame_phase) <= 2 /\ Cardinality(DOMAIN frame_run) <= 2 /\ Cardinality(DOMAIN frame_parent_loop) <= 2 /\ Cardinality(DOMAIN frame_iteration) <= 2 /\ Cardinality(DOMAIN frame_tracked_nodes) <= 2 /\ Cardinality(DOMAIN frame_ordered_nodes) <= 2 /\ Cardinality(DOMAIN frame_node_kind) <= 2 /\ Cardinality(DOMAIN frame_node_dependencies) <= 2 /\ Cardinality(DOMAIN frame_node_dependency_modes) <= 2 /\ Cardinality(DOMAIN frame_node_step_ids) <= 2 /\ Cardinality(DOMAIN frame_node_loop_ids) <= 2 /\ Cardinality(DOMAIN frame_node_status) <= 2 /\ Cardinality(DOMAIN frame_ready_queue) <= 2 /\ Cardinality(DOMAIN frame_output_recorded) <= 2 /\ Cardinality(DOMAIN frame_node_condition_results) <= 2 /\ Cardinality(DOMAIN frame_node_branches) <= 2 /\ Cardinality(DOMAIN loop_phase) <= 2 /\ Cardinality(DOMAIN loop_parent_frame) <= 2 /\ Cardinality(DOMAIN loop_parent_node) <= 2 /\ Cardinality(DOMAIN loop_definition) <= 2 /\ Cardinality(DOMAIN loop_depth) <= 2 /\ Cardinality(DOMAIN loop_stage) <= 2 /\ Cardinality(DOMAIN loop_current_iteration) <= 2 /\ Cardinality(DOMAIN loop_last_completed_iteration) <= 2 /\ Cardinality(DOMAIN loop_max_iterations) <= 2 /\ Cardinality(DOMAIN loop_active_body_frame) <= 2 /\ Cardinality(DOMAIN pending_spawn_sessions) <= 2 /\ Cardinality(member_startup_binding_requested) <= 2 /\ Cardinality(member_startup_runtime_ready) <= 2 /\ Cardinality(member_startup_ready) <= 2 /\ Cardinality(member_kickoff_pending) <= 2 /\ Cardinality(member_kickoff_starting) <= 2 /\ Cardinality(member_kickoff_callback_pending) <= 2 /\ Cardinality(member_kickoff_started) <= 2 /\ Cardinality(member_kickoff_failed) <= 2 /\ Cardinality(member_kickoff_cancelled) <= 2 /\ Cardinality(DOMAIN member_kickoff_error) <= 2 /\ Cardinality(DOMAIN member_state_markers) <= 2 /\ Cardinality(wiring_edges) <= 2 /\ Cardinality(external_peer_edges) <= 2 /\ Cardinality(DOMAIN identity_to_runtime) <= 2 /\ Cardinality(DOMAIN tasks) <= 2 /\ Cardinality(in_progress_task_ids) <= 2 /\ Cardinality(completed_task_ids) <= 2 /\ Cardinality(DOMAIN member_session_bindings) <= 2 /\ Cardinality(pending_session_ingress_detach_runtime_ids) <= 2

Spec == Init /\ [][Next]_vars

THEOREM Spec => []bindings_require_known_identity

=============================================================================
