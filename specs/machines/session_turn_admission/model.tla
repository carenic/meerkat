---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for SessionTurnAdmissionMachine.

CONSTANTS BooleanValues, NatValues, PendingContinuationDispositionValues, RuntimeKeepAlivePersistenceDecisionValues, RuntimeKeepAliveRequestValues, StartTurnDispatchAuthorizationValues, StartTurnDispositionValues, StartTurnExecutionKindValues, StartTurnPublicTerminalValues, TurnAdmissionPhaseValues, TurnAdmissionShutdownTerminalValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

OptionStartTurnPublicTerminalValues == {None} \cup {Some(x) : x \in StartTurnPublicTerminalValues}

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

VARIABLES phase, model_step_count, interrupt_pending, shutdown_pending, admission_drain_pending, last_public_terminal

vars == << phase, model_step_count, interrupt_pending, shutdown_pending, admission_drain_pending, last_public_terminal >>

prompt_has_content(prompt_trimmed_text_byte_count, prompt_non_text_block_count) == (IF (prompt_trimmed_text_byte_count > 0) THEN TRUE ELSE (prompt_non_text_block_count > 0))
is_active_phase(arg_phase) == (IF (arg_phase = "Admitted") THEN TRUE ELSE (IF (arg_phase = "Running") THEN TRUE ELSE (arg_phase = "Completing")))

Init ==
    /\ phase = "Idle"
    /\ model_step_count = 0
    /\ interrupt_pending = FALSE
    /\ shutdown_pending = FALSE
    /\ admission_drain_pending = FALSE
    /\ last_public_terminal = None

TerminalStutter ==
    /\ phase = "ShuttingDown"
    /\ UNCHANGED vars

\* Named UNCHANGED frames. One definition per distinct frame; every action
\* that leaves those variables unchanged references the definition by name.
UnchangedFrame_046c992d3dcb0dcb == UNCHANGED << shutdown_pending, admission_drain_pending, last_public_terminal >>
UnchangedFrame_055b26c6c93962ea == UNCHANGED << admission_drain_pending >>
UnchangedFrame_108875a9a5d38041 == UNCHANGED << admission_drain_pending, last_public_terminal >>
UnchangedFrame_24820eb903e12422 == UNCHANGED << interrupt_pending, shutdown_pending, last_public_terminal >>
UnchangedFrame_42fc8910cc22d6f9 == UNCHANGED << last_public_terminal >>
UnchangedFrame_46e95c01cac5058b == UNCHANGED << interrupt_pending, shutdown_pending, admission_drain_pending >>
UnchangedFrame_6346570d1f69f450 == UNCHANGED << interrupt_pending, shutdown_pending, admission_drain_pending, last_public_terminal >>
UnchangedFrame_7561b2ea569a6002 == UNCHANGED << interrupt_pending, admission_drain_pending, last_public_terminal >>
UnchangedFrame_ab70b2726ab662a3 == UNCHANGED << shutdown_pending, last_public_terminal >>

ProjectTurnAdmissionIdle ==
    /\ phase = "Idle"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ProjectTurnAdmissionAdmitted ==
    /\ phase = "Admitted"
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ProjectTurnAdmissionRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ProjectTurnAdmissionCompleting ==
    /\ phase = "Completing"
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ProjectTurnAdmissionShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ClaimTurn ==
    /\ phase = "Idle"
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE
    /\ last_public_terminal' = None
    /\ UnchangedFrame_055b26c6c93962ea


AbortClaim ==
    /\ phase = "Admitted"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE
    /\ UnchangedFrame_108875a9a5d38041


ClaimTurnShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


AbortClaimShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


BeginTurn ==
    /\ phase = "Admitted"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


BeginTurnShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveTurn ==
    /\ phase = "Running"
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


FinalizeTurnToShutdown ==
    /\ phase = "Completing"
    /\ shutdown_pending
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ admission_drain_pending' = TRUE
    /\ UnchangedFrame_ab70b2726ab662a3


FinalizeTurnToIdle ==
    /\ phase = "Completing"
    /\ (shutdown_pending = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE
    /\ UnchangedFrame_108875a9a5d38041


RequestInterruptAdmittedFirst ==
    /\ phase = "Admitted"
    /\ (interrupt_pending = FALSE)
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = TRUE
    /\ UnchangedFrame_046c992d3dcb0dcb


RequestInterruptAdmittedDuplicate ==
    /\ phase = "Admitted"
    /\ interrupt_pending
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


RequestInterruptRunningFirst ==
    /\ phase = "Running"
    /\ (interrupt_pending = FALSE)
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = TRUE
    /\ UnchangedFrame_046c992d3dcb0dcb


RequestInterruptRunningDuplicate ==
    /\ phase = "Running"
    /\ interrupt_pending
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


RequestShutdownImmediateIdle ==
    /\ phase = "Idle"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = TRUE
    /\ admission_drain_pending' = TRUE
    /\ UnchangedFrame_42fc8910cc22d6f9


RequestShutdownImmediateAdmitted ==
    /\ phase = "Admitted"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = TRUE
    /\ admission_drain_pending' = TRUE
    /\ UnchangedFrame_42fc8910cc22d6f9


RequestShutdownDeferredRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ shutdown_pending' = TRUE
    /\ UnchangedFrame_7561b2ea569a6002


RequestShutdownDeferredCompleting ==
    /\ phase = "Completing"
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ shutdown_pending' = TRUE
    /\ UnchangedFrame_7561b2ea569a6002


RequestShutdownAlreadyShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolvePendingAdmissionDrained ==
    /\ phase = "ShuttingDown"
    /\ admission_drain_pending
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ admission_drain_pending' = FALSE
    /\ UnchangedFrame_24820eb903e12422


AuthorizeSessionTeardown ==
    /\ phase = "ShuttingDown"
    /\ (admission_drain_pending = FALSE)
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


AuthorizeCancelAfterBoundaryAdmitted ==
    /\ phase = "Admitted"
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


AuthorizeStartTurnDispatchAdmitted ==
    /\ phase = "Admitted"
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


AuthorizeStartTurnDispatchShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


AuthorizeCancelAfterBoundaryRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveDispositionContentTurn(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ (execution_kind_present /\ (execution_kind = "ContentTurn"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = None
    /\ UnchangedFrame_46e95c01cac5058b


ResolveDispositionResumePendingWithBoundary(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ (execution_kind_present /\ (execution_kind = "ResumePending") /\ (pending_continuation = "RunPending"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = None
    /\ UnchangedFrame_46e95c01cac5058b


ResolveDispositionResumePendingWithoutBoundary(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ (execution_kind_present /\ (execution_kind = "ResumePending") /\ (pending_continuation = "NoPendingBoundary"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = Some("NoPendingBoundary")
    /\ UnchangedFrame_46e95c01cac5058b


ResolveDispositionDirectPrompt(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ ((execution_kind_present = FALSE) /\ prompt_has_content(prompt_trimmed_text_byte_count, prompt_non_text_block_count))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = None
    /\ UnchangedFrame_46e95c01cac5058b


ResolveDispositionDirectPending(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ ((execution_kind_present = FALSE) /\ (prompt_has_content(prompt_trimmed_text_byte_count, prompt_non_text_block_count) = FALSE) /\ (pending_continuation = "RunPending"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = None
    /\ UnchangedFrame_46e95c01cac5058b


ResolveDispositionDirectNoPending(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "Admitted"
    /\ ((execution_kind_present = FALSE) /\ (prompt_has_content(prompt_trimmed_text_byte_count, prompt_non_text_block_count) = FALSE) /\ (pending_continuation = "NoPendingBoundary"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ last_public_terminal' = Some("NoPendingBoundary")
    /\ UnchangedFrame_46e95c01cac5058b


ResolveStartTurnDispositionShuttingDown(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation) ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveRuntimeKeepAliveEnable(keep_alive_request) ==
    /\ phase = "Admitted"
    /\ (keep_alive_request = "Enable")
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveRuntimeKeepAliveDisable(keep_alive_request) ==
    /\ phase = "Admitted"
    /\ (keep_alive_request = "Disable")
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveRuntimeKeepAlivePreserve(keep_alive_request) ==
    /\ phase = "Admitted"
    /\ (keep_alive_request = "Preserve")
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveRuntimeKeepAliveShuttingDown(keep_alive_request) ==
    /\ phase = "ShuttingDown"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveLastStartTurnPublicTerminalNoPendingIdle ==
    /\ phase = "Idle"
    /\ (last_public_terminal = Some("NoPendingBoundary"))
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveLastStartTurnPublicTerminalNoPendingAdmitted ==
    /\ phase = "Admitted"
    /\ (last_public_terminal = Some("NoPendingBoundary"))
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveLastStartTurnPublicTerminalNoPendingRunning ==
    /\ phase = "Running"
    /\ (last_public_terminal = Some("NoPendingBoundary"))
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveLastStartTurnPublicTerminalNoPendingCompleting ==
    /\ phase = "Completing"
    /\ (last_public_terminal = Some("NoPendingBoundary"))
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


ResolveLastStartTurnPublicTerminalNoPendingShuttingDown ==
    /\ phase = "ShuttingDown"
    /\ (last_public_terminal = Some("NoPendingBoundary"))
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_6346570d1f69f450


Next ==
    \/ ProjectTurnAdmissionIdle
    \/ ProjectTurnAdmissionAdmitted
    \/ ProjectTurnAdmissionRunning
    \/ ProjectTurnAdmissionCompleting
    \/ ProjectTurnAdmissionShuttingDown
    \/ ClaimTurn
    \/ AbortClaim
    \/ ClaimTurnShuttingDown
    \/ AbortClaimShuttingDown
    \/ BeginTurn
    \/ BeginTurnShuttingDown
    \/ ResolveTurn
    \/ FinalizeTurnToShutdown
    \/ FinalizeTurnToIdle
    \/ RequestInterruptAdmittedFirst
    \/ RequestInterruptAdmittedDuplicate
    \/ RequestInterruptRunningFirst
    \/ RequestInterruptRunningDuplicate
    \/ RequestShutdownImmediateIdle
    \/ RequestShutdownImmediateAdmitted
    \/ RequestShutdownDeferredRunning
    \/ RequestShutdownDeferredCompleting
    \/ RequestShutdownAlreadyShuttingDown
    \/ ResolvePendingAdmissionDrained
    \/ AuthorizeSessionTeardown
    \/ AuthorizeCancelAfterBoundaryAdmitted
    \/ AuthorizeStartTurnDispatchAdmitted
    \/ AuthorizeStartTurnDispatchShuttingDown
    \/ AuthorizeCancelAfterBoundaryRunning
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionContentTurn(TRUE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionResumePendingWithBoundary(TRUE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionResumePendingWithoutBoundary(TRUE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionDirectPrompt(FALSE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionDirectPending(FALSE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveDispositionDirectNoPending(FALSE, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E execution_kind_present \in BOOLEAN : \E execution_kind \in StartTurnExecutionKindValues : \E prompt_trimmed_text_byte_count \in 0..2 : \E prompt_non_text_block_count \in 0..2 : \E pending_continuation \in PendingContinuationDispositionValues : ResolveStartTurnDispositionShuttingDown(execution_kind_present, execution_kind, prompt_trimmed_text_byte_count, prompt_non_text_block_count, pending_continuation)
    \/ \E keep_alive_request \in RuntimeKeepAliveRequestValues : ResolveRuntimeKeepAliveEnable(keep_alive_request)
    \/ \E keep_alive_request \in RuntimeKeepAliveRequestValues : ResolveRuntimeKeepAliveDisable(keep_alive_request)
    \/ \E keep_alive_request \in RuntimeKeepAliveRequestValues : ResolveRuntimeKeepAlivePreserve(keep_alive_request)
    \/ \E keep_alive_request \in RuntimeKeepAliveRequestValues : ResolveRuntimeKeepAliveShuttingDown(keep_alive_request)
    \/ ResolveLastStartTurnPublicTerminalNoPendingIdle
    \/ ResolveLastStartTurnPublicTerminalNoPendingAdmitted
    \/ ResolveLastStartTurnPublicTerminalNoPendingRunning
    \/ ResolveLastStartTurnPublicTerminalNoPendingCompleting
    \/ ResolveLastStartTurnPublicTerminalNoPendingShuttingDown
    \/ TerminalStutter

shutdown_phase_is_not_active == (IF (phase # "ShuttingDown") THEN TRUE ELSE (is_active_phase(phase) = FALSE))
drain_obligation_only_while_shutting_down == (IF (admission_drain_pending = FALSE) THEN TRUE ELSE (phase = "ShuttingDown"))

CiStateConstraint == /\ model_step_count <= 6
DeepStateConstraint == /\ model_step_count <= 8

Spec == Init /\ [][Next]_vars

THEOREM Spec => []shutdown_phase_is_not_active
THEOREM Spec => []drain_obligation_only_while_shutting_down

=============================================================================
