---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for WorkExecutionLifecycleMachine.

CONSTANTS BooleanValues, NatValues, StringValues, WorkExecutionEvidenceKindValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

OptionStringValues == {None} \cup {Some(x) : x \in StringValues}
OptionWorkExecutionEvidenceKindValues == {None} \cup {Some(x) : x \in WorkExecutionEvidenceKindValues}

MapLookup(map, key) == IF key \in DOMAIN map THEN map[key] ELSE None
MapSet(map, key, value) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN value ELSE map[x]]
MapIncrement(map, key, amount) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN (IF key \in DOMAIN map THEN map[key] ELSE 0) + amount ELSE map[x]]
MapDecrement(map, key, amount) == [x \in DOMAIN map \cup {key} |-> IF x = key THEN (IF key \in DOMAIN map THEN map[key] ELSE 0) - amount ELSE map[x]]
MapRemove(map, key) == [x \in DOMAIN map \ {key} |-> map[x]]
StartsWith(seq, prefix) == /\ Len(prefix) <= Len(seq) /\ SubSeq(seq, 1, Len(prefix)) = prefix
SeqElements(seq) == {seq[i] : i \in 1..Len(seq)}
Count(seq, value) == Cardinality({i \in DOMAIN seq : seq[i] = value})
RECURSIVE SeqRemove(_, _)
SeqRemove(seq, value) == IF Len(seq) = 0 THEN <<>> ELSE IF Head(seq) = value THEN SeqRemove(Tail(seq), value) ELSE <<Head(seq)>> \o SeqRemove(Tail(seq), value)
RECURSIVE SeqRemoveAll(_, _)
SeqRemoveAll(seq, values) == IF Len(values) = 0 THEN seq ELSE SeqRemoveAll(SeqRemove(seq, Head(values)), Tail(values))

VARIABLES phase, model_step_count, binding_id, run_id, revision, last_failure_detail, evidence_kind

vars == << phase, model_step_count, binding_id, run_id, revision, last_failure_detail, evidence_kind >>

Init ==
    /\ phase = "Absent"
    /\ model_step_count = 0
    /\ binding_id = ""
    /\ run_id = ""
    /\ revision = 0
    /\ last_failure_detail = None
    /\ evidence_kind = None

TerminalStutter ==
    /\ phase = "FlowFailed" \/ phase = "FlowCanceled" \/ phase = "EvidenceProjected" \/ phase = "WorkClosed" \/ phase = "LaunchFailed"
    /\ UNCHANGED vars

BindExecution(arg_binding_id, arg_run_id) ==
    /\ phase = "Absent"
    /\ ((arg_binding_id # "") /\ (arg_run_id # ""))
    /\ phase' = "LaunchRequested"
    /\ model_step_count' = model_step_count + 1
    /\ binding_id' = arg_binding_id
    /\ run_id' = arg_run_id
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = None
    /\ evidence_kind' = None


RecoverLaunchRequest ==
    /\ phase = "LaunchRequested"
    /\ phase' = "LaunchRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverUncertainLaunch ==
    /\ phase = "LaunchUncertain"
    /\ phase' = "LaunchUncertain"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverQuarantinedLaunch ==
    /\ phase = "LaunchQuarantined"
    /\ phase' = "LaunchQuarantined"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverEvidenceProjection ==
    /\ phase = "EvidenceProjectionRequested"
    /\ phase' = "EvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverWorkClosure ==
    /\ phase = "WorkClosureRequested"
    /\ phase' = "WorkClosureRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverFlowFailureEvidenceProjection ==
    /\ phase = "FailureEvidenceProjectionRequested"
    /\ phase' = "FailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverFlowCancellationEvidenceProjection ==
    /\ phase = "CancellationEvidenceProjectionRequested"
    /\ phase' = "CancellationEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverLaunchFailureEvidenceProjection ==
    /\ phase = "LaunchFailureEvidenceProjectionRequested"
    /\ phase' = "LaunchFailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverFlowFailure ==
    /\ phase = "FlowFailed"
    /\ phase' = "FlowFailed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverFlowCancellation ==
    /\ phase = "FlowCanceled"
    /\ phase' = "FlowCanceled"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverEvidenceProjected ==
    /\ phase = "EvidenceProjected"
    /\ phase' = "EvidenceProjected"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverClosedWork ==
    /\ phase = "WorkClosed"
    /\ phase' = "WorkClosed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


RecoverLaunchFailure ==
    /\ phase = "LaunchFailed"
    /\ phase' = "LaunchFailed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


AcceptFlowLaunch ==
    /\ phase = "LaunchRequested" \/ phase = "LaunchUncertain" \/ phase = "LaunchQuarantined"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = None
    /\ evidence_kind' = None
    /\ UNCHANGED << binding_id, run_id >>


ObserveRunningFlow ==
    /\ phase = "Running" \/ phase = "LaunchRequested" \/ phase = "LaunchUncertain" \/ phase = "LaunchQuarantined"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = None
    /\ evidence_kind' = None
    /\ UNCHANGED << binding_id, run_id >>


ObserveCompletedFlow ==
    /\ phase = "Running" \/ phase = "LaunchRequested" \/ phase = "LaunchUncertain" \/ phase = "LaunchQuarantined"
    /\ phase' = "EvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = None
    /\ evidence_kind' = Some("Completed")
    /\ UNCHANGED << binding_id, run_id >>


ObserveFailedFlow(detail) ==
    /\ phase = "Running" \/ phase = "LaunchRequested" \/ phase = "LaunchUncertain" \/ phase = "LaunchQuarantined"
    /\ phase' = "FailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = detail
    /\ evidence_kind' = Some("Failed")
    /\ UNCHANGED << binding_id, run_id >>


ObserveCanceledFlow(detail) ==
    /\ phase = "Running" \/ phase = "LaunchRequested" \/ phase = "LaunchUncertain" \/ phase = "LaunchQuarantined"
    /\ phase' = "CancellationEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = detail
    /\ evidence_kind' = Some("Canceled")
    /\ UNCHANGED << binding_id, run_id >>


ObserveLostRun(detail) ==
    /\ phase = "Running"
    /\ phase' = "FailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ evidence_kind' = Some("RunLost")
    /\ UNCHANGED << binding_id, run_id >>


ObserveLostCompletedRunBeforeEvidence(detail) ==
    /\ phase = "EvidenceProjectionRequested"
    /\ phase' = "FailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ evidence_kind' = Some("RunLost")
    /\ UNCHANGED << binding_id, run_id >>


RecordUncertainLaunch(detail) ==
    /\ phase = "LaunchRequested"
    /\ phase' = "LaunchUncertain"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ evidence_kind' = None
    /\ UNCHANGED << binding_id, run_id >>


QuarantineLaunch(detail) ==
    /\ phase = "LaunchRequested" \/ phase = "LaunchUncertain"
    /\ phase' = "LaunchQuarantined"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ evidence_kind' = None
    /\ UNCHANGED << binding_id, run_id >>


FailLaunch(detail) ==
    /\ phase = "LaunchRequested" \/ phase = "LaunchUncertain"
    /\ phase' = "LaunchFailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ evidence_kind' = Some("LaunchFailed")
    /\ UNCHANGED << binding_id, run_id >>


CommitLaunchFailureEvidenceProjection ==
    /\ phase = "LaunchFailureEvidenceProjectionRequested"
    /\ phase' = "LaunchFailed"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ UNCHANGED << binding_id, run_id, last_failure_detail, evidence_kind >>


CommitEvidenceProjection ==
    /\ phase = "EvidenceProjectionRequested"
    /\ phase' = "WorkClosureRequested"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ UNCHANGED << binding_id, run_id, last_failure_detail, evidence_kind >>


CommitFlowFailureEvidenceProjection ==
    /\ phase = "FailureEvidenceProjectionRequested"
    /\ phase' = "FlowFailed"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ UNCHANGED << binding_id, run_id, last_failure_detail, evidence_kind >>


CommitFlowCancellationEvidenceProjection ==
    /\ phase = "CancellationEvidenceProjectionRequested"
    /\ phase' = "FlowCanceled"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ UNCHANGED << binding_id, run_id, last_failure_detail, evidence_kind >>


CommitWorkClosure ==
    /\ phase = "WorkClosureRequested"
    /\ phase' = "WorkClosed"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = None
    /\ UNCHANGED << binding_id, run_id, evidence_kind >>


RecordWorkClosureRefusal(detail) ==
    /\ phase = "WorkClosureRequested"
    /\ phase' = "EvidenceProjected"
    /\ model_step_count' = model_step_count + 1
    /\ revision' = (revision) + 1
    /\ last_failure_detail' = Some(detail)
    /\ UNCHANGED << binding_id, run_id, evidence_kind >>


ClassifyRetryEligibilityTerminalFlowFailed ==
    /\ phase = "FlowFailed"
    /\ phase' = "FlowFailed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityTerminalFlowCanceled ==
    /\ phase = "FlowCanceled"
    /\ phase' = "FlowCanceled"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityTerminalEvidenceProjected ==
    /\ phase = "EvidenceProjected"
    /\ phase' = "EvidenceProjected"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityTerminalWorkClosed ==
    /\ phase = "WorkClosed"
    /\ phase' = "WorkClosed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityTerminalLaunchFailed ==
    /\ phase = "LaunchFailed"
    /\ phase' = "LaunchFailed"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveAbsent ==
    /\ phase = "Absent"
    /\ phase' = "Absent"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveLaunchRequested ==
    /\ phase = "LaunchRequested"
    /\ phase' = "LaunchRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveLaunchUncertain ==
    /\ phase = "LaunchUncertain"
    /\ phase' = "LaunchUncertain"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveLaunchQuarantined ==
    /\ phase = "LaunchQuarantined"
    /\ phase' = "LaunchQuarantined"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveEvidenceProjectionRequested ==
    /\ phase = "EvidenceProjectionRequested"
    /\ phase' = "EvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveFailureEvidenceProjectionRequested ==
    /\ phase = "FailureEvidenceProjectionRequested"
    /\ phase' = "FailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveCancellationEvidenceProjectionRequested ==
    /\ phase = "CancellationEvidenceProjectionRequested"
    /\ phase' = "CancellationEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveLaunchFailureEvidenceProjectionRequested ==
    /\ phase = "LaunchFailureEvidenceProjectionRequested"
    /\ phase' = "LaunchFailureEvidenceProjectionRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


ClassifyRetryEligibilityLiveWorkClosureRequested ==
    /\ phase = "WorkClosureRequested"
    /\ phase' = "WorkClosureRequested"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << binding_id, run_id, revision, last_failure_detail, evidence_kind >>


Next ==
    \/ \E arg_binding_id \in StringValues : \E arg_run_id \in StringValues : BindExecution(arg_binding_id, arg_run_id)
    \/ RecoverLaunchRequest
    \/ RecoverUncertainLaunch
    \/ RecoverQuarantinedLaunch
    \/ RecoverRunning
    \/ RecoverEvidenceProjection
    \/ RecoverWorkClosure
    \/ RecoverFlowFailureEvidenceProjection
    \/ RecoverFlowCancellationEvidenceProjection
    \/ RecoverLaunchFailureEvidenceProjection
    \/ RecoverFlowFailure
    \/ RecoverFlowCancellation
    \/ RecoverEvidenceProjected
    \/ RecoverClosedWork
    \/ RecoverLaunchFailure
    \/ AcceptFlowLaunch
    \/ ObserveRunningFlow
    \/ ObserveCompletedFlow
    \/ \E detail \in OptionStringValues : ObserveFailedFlow(detail)
    \/ \E detail \in OptionStringValues : ObserveCanceledFlow(detail)
    \/ \E detail \in StringValues : ObserveLostRun(detail)
    \/ \E detail \in StringValues : ObserveLostCompletedRunBeforeEvidence(detail)
    \/ \E detail \in StringValues : RecordUncertainLaunch(detail)
    \/ \E detail \in StringValues : QuarantineLaunch(detail)
    \/ \E detail \in StringValues : FailLaunch(detail)
    \/ CommitLaunchFailureEvidenceProjection
    \/ CommitEvidenceProjection
    \/ CommitFlowFailureEvidenceProjection
    \/ CommitFlowCancellationEvidenceProjection
    \/ CommitWorkClosure
    \/ \E detail \in StringValues : RecordWorkClosureRefusal(detail)
    \/ ClassifyRetryEligibilityTerminalFlowFailed
    \/ ClassifyRetryEligibilityTerminalFlowCanceled
    \/ ClassifyRetryEligibilityTerminalEvidenceProjected
    \/ ClassifyRetryEligibilityTerminalWorkClosed
    \/ ClassifyRetryEligibilityTerminalLaunchFailed
    \/ ClassifyRetryEligibilityLiveAbsent
    \/ ClassifyRetryEligibilityLiveLaunchRequested
    \/ ClassifyRetryEligibilityLiveLaunchUncertain
    \/ ClassifyRetryEligibilityLiveLaunchQuarantined
    \/ ClassifyRetryEligibilityLiveRunning
    \/ ClassifyRetryEligibilityLiveEvidenceProjectionRequested
    \/ ClassifyRetryEligibilityLiveFailureEvidenceProjectionRequested
    \/ ClassifyRetryEligibilityLiveCancellationEvidenceProjectionRequested
    \/ ClassifyRetryEligibilityLiveLaunchFailureEvidenceProjectionRequested
    \/ ClassifyRetryEligibilityLiveWorkClosureRequested
    \/ TerminalStutter

identified_after_bind == (IF (phase = "Absent") THEN TRUE ELSE ((binding_id # "") /\ (run_id # "")))
evidence_projection_is_typed == (IF ((phase # "EvidenceProjectionRequested") /\ (phase # "FailureEvidenceProjectionRequested") /\ (phase # "CancellationEvidenceProjectionRequested") /\ (phase # "LaunchFailureEvidenceProjectionRequested")) THEN TRUE ELSE (evidence_kind # None))

CiStateConstraint == /\ model_step_count <= 6
DeepStateConstraint == /\ model_step_count <= 8

Spec == Init /\ [][Next]_vars

THEOREM Spec => []identified_after_bind
THEOREM Spec => []evidence_projection_is_typed

=============================================================================
