---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for SessionTurnAdmissionMachine.

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

VARIABLES phase, model_step_count, interrupt_pending, shutdown_pending

vars == << phase, model_step_count, interrupt_pending, shutdown_pending >>

Init ==
    /\ phase = "Idle"
    /\ model_step_count = 0
    /\ interrupt_pending = FALSE
    /\ shutdown_pending = FALSE

TerminalStutter ==
    /\ phase = "ShuttingDown"
    /\ UNCHANGED vars

RequestStartTurn ==
    /\ phase = "Idle"
    /\ phase' = "Admitted"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE


AbortAdmittedTurn ==
    /\ phase = "Admitted"
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE


BeginRun ==
    /\ phase = "Admitted"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << interrupt_pending, shutdown_pending >>


ShutdownFromAdmitted ==
    /\ phase = "Admitted"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = TRUE


ResolveRun ==
    /\ phase = "Running"
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ UNCHANGED << interrupt_pending, shutdown_pending >>


RequestInterrupt ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = TRUE
    /\ UNCHANGED << shutdown_pending >>


RequestShutdownFromRunning ==
    /\ phase = "Running"
    /\ phase' = "Running"
    /\ model_step_count' = model_step_count + 1
    /\ shutdown_pending' = TRUE
    /\ UNCHANGED << interrupt_pending >>


RequestShutdownFromCompleting ==
    /\ phase = "Completing"
    /\ phase' = "Completing"
    /\ model_step_count' = model_step_count + 1
    /\ shutdown_pending' = TRUE
    /\ UNCHANGED << interrupt_pending >>


FinalizeTurnToIdle ==
    /\ phase = "Completing"
    /\ (shutdown_pending = FALSE)
    /\ phase' = "Idle"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = FALSE


FinalizeTurnToShuttingDown ==
    /\ phase = "Completing"
    /\ (shutdown_pending = TRUE)
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ UNCHANGED << shutdown_pending >>


RequestShutdownFromIdle ==
    /\ phase = "Idle"
    /\ phase' = "ShuttingDown"
    /\ model_step_count' = model_step_count + 1
    /\ interrupt_pending' = FALSE
    /\ shutdown_pending' = TRUE


Next ==
    \/ RequestStartTurn
    \/ AbortAdmittedTurn
    \/ BeginRun
    \/ ShutdownFromAdmitted
    \/ ResolveRun
    \/ RequestInterrupt
    \/ RequestShutdownFromRunning
    \/ RequestShutdownFromCompleting
    \/ FinalizeTurnToIdle
    \/ FinalizeTurnToShuttingDown
    \/ RequestShutdownFromIdle
    \/ TerminalStutter

interrupt_pending_only_while_active == ((interrupt_pending = FALSE) \/ ((phase = "Running") \/ (phase = "Completing")))

CiStateConstraint == /\ model_step_count <= 6
DeepStateConstraint == /\ model_step_count <= 8

Spec == Init /\ [][Next]_vars

THEOREM Spec => []interrupt_pending_only_while_active

=============================================================================
