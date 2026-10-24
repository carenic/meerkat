---- MODULE materialization_rollback_keep_audit ----
\* Hand-written bounded audit of the Keep disposition: the rollback of a
\* materialization that was refused before it served (a durable tail held for
\* recovery, a failed agent build) keeps every input its registration
\* recovered from the store exactly as the store holds it, and still stops an
\* input admitted while the registration stood.
\*
\* This audit keeps the generated MeerkatMachine model unchanged and explores
\* three processes. Each restart is a new process: every generated variable
\* returns to its initial value (AuditFreshMachine, derived from the
\* generated Init on every run by materialization_rollback_keep_audit.sh).
\*
\* Process 1 registers one session, admits A, attaches, stages A into a run,
\* admits B behind that run (every kind and durability) and crashes, so A
\* (Staged) and B (Queued) are its durable non-terminal rows. Process 2
\* registers and recovers each durable row through the generated recovery
\* arms (the durability classifier discards an Ephemeral row, and the staged
\* A is queued again), may admit a new input C, and its materialization is
\* refused before it serves: the host rolls the registration back with an
\* unregister that begins with Keep, or, as the control, with Stop. The
\* rollback's stop is one driver-lock transaction, as in the shell: it
\* classifies each non-terminal input before the executor's exit (an input
\* answered Abandon gets the runless carrier) and again after it, ends the
\* inputs answered Abandon as stopped, and commits. Process 3 registers,
\* recovers the durable rows again, attaches and stages each recovered input
\* into a run. Rows recover, and inputs are classified and ended, in one
\* fixed order, so the interleavings of independent rows do not multiply the
\* space. Properties (all invariants):
\*
\*   AuditRollbackVerdictIsAFunction: during the rollback's stop exactly one
\*     verdict arm (Retain, Keep or Abandon) answers each input still to
\*     classify.
\*   AuditKeepEndsNoRecoveredInput: under Keep, no input process 2 recovered
\*     is answered Abandon, gets a runless carrier or ends stopped.
\*   AuditKeepStopsWhatItAdmitted: an input admitted in process 2 is never
\*     answered Keep and never runs in process 3.
\*   AuditKeepOnlyUnderKeep: every Keep answer came from an unregister the
\*     host began with Keep.
\*   AuditKeptInputSurvivesTheRollback: an input the rollback kept is still
\*     non-terminal once the stop committed, so process 3 recovers it.
\*
\* AuditWitness* operators are reachability goals the runner checks as
\* expected violations, so the invariants are not vacuous. The runner's
\* --mutants mode seeds four defects into a copy of the generated model and
\* requires TLC to refuse each with the invariant it breaks: the machine
\* without the Keep arm (Keep stops like Stop), a Keep arm that ignores the
\* recovery witness, an Abandon arm that overlaps Keep, and a Keep arm that
\* ignores the disposition.
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* materialization_rollback_keep_audit.sh. No audit step repeats, so the
\* reachable space is finite: the lane's bound of 50 steps explores all of it
\* (a deeper bound finds no new state), and 40 steps reach the deepest
\* witness. Check that by hand with:
\*   specs/machines/meerkat_machine/materialization_rollback_keep_audit.sh 60
\*
\* Scope limits: the requeue mark, the waiter answers, the runless carriers
\* and the durable rows are shell, so the carrier set here is the shell's rule
\* (the inputs answered Abandon) and a kept row is the recovered row
\* unchanged. The meerkat facade rows pin the shell: a refused restart keeps
\* the stranded input, the dry run still names it, and the next start runs
\* it once.
EXTENDS materialization_rollback_keep_fresh

CONSTANTS AuditMaxSteps

VARIABLES
    audit_stage,          \* which part of the three-process run this is
    audit_facts,          \* each admitted input's kind and durability
    audit_disposition,    \* the disposition process 2's rollback began with
    audit_c_planned,      \* process 2's admission of C was planned
    audit_admitted_p2,    \* inputs admitted while process 2's registration stood
    audit_pending,        \* inputs the open stop pass still classifies
    audit_pre_verdict,    \* the stop's verdicts before the exit (carriers)
    audit_post_verdict,   \* the stop's verdicts after the exit (cleanup)
    audit_abandoned,      \* Abandon answers the stop's cleanup ended
    audit_kept,           \* inputs the committed stop kept
    audit_carriers,       \* inputs that got the stop's runless carrier
    audit_stopped,        \* inputs the stop's cleanup ended Stopped
    audit_rows,           \* the durable non-terminal rows the process recovers
    audit_recovery,       \* each recovered row's progress through recovery
    audit_recovered_p2,   \* inputs process 2 recovered from the store
    audit_staged_p3       \* inputs staged into a run of process 3

setupVars == <<audit_facts, audit_disposition, audit_c_planned, audit_admitted_p2>>
regionVars == <<audit_pending, audit_pre_verdict, audit_post_verdict, audit_abandoned>>
historyVars == <<audit_kept, audit_carriers, audit_stopped>>
restartVars == <<audit_rows, audit_recovery, audit_recovered_p2, audit_staged_p3>>
auditVars == <<audit_stage, setupVars, regionVars, historyVars, restartVars>>

AuditSession == "sessionid_1"
AuditA == "input_1"
AuditB == "input_2"
AuditC == "input_3"
\* The fixed order rows recover and inputs are classified and ended in.
AuditOrder == <<AuditA, AuditB, AuditC>>
AuditInputs == {AuditA, AuditB, AuditC}
AuditRun == "runid_1"
AuditKinds == {"Prompt", "ExternalEvent"}
TerminalPhases == {"Consumed", "Superseded", "Coalesced", "Abandoned"}
NoVerdict == [x \in {} |-> "Abandon"]

PhaseOf(i) == IF i \in DOMAIN input_phases THEN input_phases[i] ELSE "Absent"
NonTerminal(i) == i \in DOMAIN input_phases /\ input_phases[i] \notin TerminalPhases
NonTerminalInputs == {i \in AuditInputs : NonTerminal(i)}
Attempts(i) == IF i \in DOMAIN input_attempt_counts THEN input_attempt_counts[i] ELSE 0
Lookup(map, i, default) == IF i \in DOMAIN map THEN map[i] ELSE default
Answered(verdicts, v) == {i \in DOMAIN verdicts : verdicts[i] = v}
\* The first input of S in the fixed order.
FirstOf(S) == AuditOrder[CHOOSE n \in 1..Len(AuditOrder) :
                            /\ AuditOrder[n] \in S
                            /\ \A m \in 1..(n - 1) : AuditOrder[m] \notin S]

\* ---- generated arms, one disjunct per phase ----

RetainArm(i, d, dir, obj) ==
    \/ ClassifyStopInputDispositionRetainInitializing(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainIdle(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainAttached(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainRunning(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainRetired(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainStopped(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableInitializing(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableIdle(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableAttached(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableRunning(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableRetired(i, d, dir, obj)
    \/ ClassifyStopInputDispositionRetainResumableStopped(i, d, dir, obj)

KeepArm(i, d, dir, obj) ==
    \/ ClassifyStopInputDispositionKeepInitializing(i, d, dir, obj)
    \/ ClassifyStopInputDispositionKeepIdle(i, d, dir, obj)
    \/ ClassifyStopInputDispositionKeepAttached(i, d, dir, obj)
    \/ ClassifyStopInputDispositionKeepRunning(i, d, dir, obj)
    \/ ClassifyStopInputDispositionKeepRetired(i, d, dir, obj)
    \/ ClassifyStopInputDispositionKeepStopped(i, d, dir, obj)

AbandonArm(i, d, dir, obj) ==
    \/ ClassifyStopInputDispositionAbandonInitializing(i, d, dir, obj)
    \/ ClassifyStopInputDispositionAbandonIdle(i, d, dir, obj)
    \/ ClassifyStopInputDispositionAbandonAttached(i, d, dir, obj)
    \/ ClassifyStopInputDispositionAbandonRunning(i, d, dir, obj)
    \/ ClassifyStopInputDispositionAbandonRetired(i, d, dir, obj)
    \/ ClassifyStopInputDispositionAbandonStopped(i, d, dir, obj)

AbandonAny(i, reason, n) ==
    \/ AbandonInputIdle(i, reason, n)
    \/ AbandonInputAttached(i, reason, n)
    \/ AbandonInputRunning(i, reason, n)
    \/ AbandonInputRetired(i, reason, n)
    \/ AbandonInputStopped(i, reason, n)

AdmitPlan(i, kind) ==
    \/ ResolveAdmissionPlanDefaultQueueKindIdle(i, None, None, kind, None, "Ordinary", "InTurnEligible", FALSE, None, FALSE, FALSE, FALSE)
    \/ ResolveAdmissionPlanDefaultQueueKindRunning(i, None, None, kind, None, "Ordinary", "InTurnEligible", FALSE, None, TRUE, FALSE, FALSE)

\* The stop classification of input i under its admission facts. A shell
\* asks durability, directedness and the objective binding; the inputs here
\* are undirected and bound to no objective.
Classify(i, verdicts) ==
    LET f == audit_facts[i] IN
    \/ RetainArm(i, f.durability, FALSE, FALSE) /\ verdicts' = verdicts @@ (i :> "Retain")
    \/ KeepArm(i, f.durability, FALSE, FALSE) /\ verdicts' = verdicts @@ (i :> "Keep")
    \/ AbandonArm(i, f.durability, FALSE, FALSE) /\ verdicts' = verdicts @@ (i :> "Abandon")

AuditUnchanged == UNCHANGED auditVars

\* ---- process 1 ----

\* Scripted start: register, admit A, attach, stage A into a run, admit B
\* behind the run.
AuditSetup ==
    /\ audit_stage = "Setup"
    /\ \/ model_step_count = 0 /\ Initialize /\ AuditUnchanged
       \/ model_step_count = 1 /\ RegisterSessionIdle(AuditSession, None, {}) /\ AuditUnchanged
       \/ model_step_count = 2 /\ AdmitPlan(AuditA, "Prompt") /\ AuditUnchanged
       \/ model_step_count = 3 /\ QueueAcceptedIdle(AuditA) /\ AuditUnchanged
       \/ model_step_count = 4 /\ EnsureSessionWithExecutorIdle(AuditSession) /\ AuditUnchanged
       \/ model_step_count = 5 /\ PrepareAttached(AuditSession, AuditRun) /\ AuditUnchanged
       \/ model_step_count = 6 /\ StageForRunRunning(AuditA, AuditRun) /\ AuditUnchanged
       \/ model_step_count = 7
          /\ \E kind \in AuditKinds, d \in InputDurabilityKindValues :
                /\ AdmitPlan(AuditB, kind)
                /\ audit_facts' = audit_facts @@ (AuditB :> [kind |-> kind, durability |-> d])
          /\ UNCHANGED <<audit_stage, audit_disposition, audit_c_planned, audit_admitted_p2,
                         regionVars, historyVars, restartVars>>
       \/ model_step_count = 8
          /\ QueueAcceptedRunning(AuditB)
          /\ audit_stage' = "P1"
          /\ UNCHANGED <<setupVars, regionVars, historyVars, restartVars>>

RowOf(i) ==
    [phase |-> input_phases[i],
     attempts |-> Attempts(i),
     seq |-> Lookup(input_admission_seq, i, 0),
     lane |-> Lookup(input_recovery_lanes, i, "Queue"),
     boundary |-> Lookup(input_runtime_boundary, i, "RunStart"),
     exec |-> Lookup(input_runtime_execution_kind, i, "ContentTurn"),
     is_prompt |-> Lookup(input_is_prompt, i, FALSE)]

\* A new process: the durable non-terminal rows, every generated variable at
\* its initial value.
Restart(next_stage) ==
    /\ AuditFreshMachine
    /\ audit_rows' = [i \in NonTerminalInputs |-> RowOf(i)]
    /\ audit_recovery' = [i \in NonTerminalInputs |-> "Durability"]
    /\ audit_stage' = next_stage

\* Process 1 crashes with A staged into its run and B queued behind it.
AuditCrashProcess1 ==
    /\ audit_stage = "P1"
    /\ Restart("P2Start")
    /\ UNCHANGED <<setupVars, regionVars, historyVars, audit_recovered_p2, audit_staged_p3>>

StartProcess(start_stage, recover_stage) ==
    /\ audit_stage = start_stage
    /\ \/ Initialize /\ AuditUnchanged
       \/ /\ RegisterSessionIdle(AuditSession, None, {})
          /\ audit_stage' = recover_stage
          /\ UNCHANGED <<setupVars, regionVars, historyVars, restartVars>>

AuditStartProcess2 == StartProcess("P2Start", "P2Recover")
AuditStartProcess3 == StartProcess("P3Start", "P3Recover")

RecoveryStep(i, next) ==
    /\ audit_recovery' = [audit_recovery EXCEPT ![i] = next]
    /\ UNCHANGED <<audit_stage, setupVars, regionVars, historyVars, audit_rows,
                   audit_recovered_p2, audit_staged_p3>>

Unrecovered == {i \in DOMAIN audit_recovery : audit_recovery[i] \notin {"Recovered", "Discarded"}}

\* Cold recovery of each durable non-terminal row through the generated arms,
\* one row at a time in the fixed order: the durability classifier (an
\* Ephemeral row is discarded), the normalization classifier for the row's
\* phase (a Staged row is queued again), then the admitted input and its
\* Queued lifecycle.
AuditRecoverRow ==
    /\ audit_stage \in {"P2Recover", "P3Recover"}
    /\ Unrecovered # {}
    /\ LET i == FirstOf(Unrecovered)
           r == audit_rows[i]
           f == audit_facts[i]
       IN
       \/ /\ audit_recovery[i] = "Durability"
          /\ \/ ClassifyRecoveredInputDurabilityRetainDurableDerivedOrMissingIdle(i, f.durability)
                /\ RecoveryStep(i, "Normalize")
             \/ ClassifyRecoveredInputDurabilityDiscardEphemeralIdle(i, f.durability)
                /\ RecoveryStep(i, "Discarded")
       \/ /\ audit_recovery[i] = "Normalize"
          /\ \/ r.phase = "Queued" /\ NormalizeRecoveredInputQueuedIdle(i, r.phase, None, FALSE)
             \/ r.phase = "Staged" /\ NormalizeRecoveredInputStagedIdle(i, r.phase, None, FALSE)
          /\ RecoveryStep(i, "Admit")
       \/ /\ audit_recovery[i] = "Admit"
          /\ RecoverAdmittedInputIdle(i, f.kind, r.boundary, r.exec, None, r.lane)
          /\ RecoveryStep(i, "Lifecycle")
       \/ /\ audit_recovery[i] = "Lifecycle"
          /\ RecoverInputLifecycleIdle(i, "Queued", None, None, None, None, 0,
                r.attempts, None, None, Some(r.seq), None, Some(r.lane),
                Some(r.lane), Some(r.boundary), Some(r.exec), None, r.is_prompt)
          /\ RecoveryStep(i, "Recovered")

RecoveredRows == {i \in DOMAIN audit_recovery : audit_recovery[i] = "Recovered"}

AuditRecoveryDone ==
    /\ audit_stage \in {"P2Recover", "P3Recover"}
    /\ Unrecovered = {}
    /\ audit_stage' = IF audit_stage = "P2Recover" THEN "P2Ready" ELSE "P3Serve"
    /\ audit_recovered_p2' = IF audit_stage = "P2Recover" THEN RecoveredRows
                                                          ELSE audit_recovered_p2
    /\ UNCHANGED vars
    /\ UNCHANGED <<setupVars, regionVars, historyVars, audit_rows, audit_recovery,
                   audit_staged_p3>>

\* ---- process 2: refused before it serves ----

\* A caller admits C while the registration stands (once).
AuditAdmitInProcess2 ==
    /\ audit_stage = "P2Ready"
    /\ \/ /\ ~audit_c_planned
          /\ AdmitPlan(AuditC, "Prompt")
          /\ audit_c_planned' = TRUE
          /\ UNCHANGED <<audit_stage, audit_facts, audit_disposition, audit_admitted_p2,
                         regionVars, historyVars, restartVars>>
       \/ /\ audit_c_planned
          /\ audit_admitted_p2 = {}
          /\ QueueAcceptedIdle(AuditC)
          /\ audit_admitted_p2' = {AuditC}
          /\ UNCHANGED <<audit_stage, audit_facts, audit_disposition, audit_c_planned,
                         regionVars, historyVars, restartVars>>

\* The materialization is refused: the host rolls its registration back with
\* an unregister that begins with Keep, or, as the control, with Stop. The
\* registration never attached an executor, so its runtime is Idle.
AuditBeginRollback ==
    /\ audit_stage = "P2Ready"
    /\ audit_c_planned => audit_admitted_p2 # {}
    /\ \E disp \in {"Keep", "Stop"} :
          /\ BeginUnregisterSessionIdle(AuditSession, None, None, None, None, disp, 0)
          /\ audit_disposition' = disp
    /\ audit_stage' = "P2Draining"
    /\ UNCHANGED <<audit_facts, audit_c_planned, audit_admitted_p2, regionVars, historyVars,
                   restartVars>>

\* The rollback's stop transaction. The open step is the shell taking the
\* driver lock.
AuditOpenStop ==
    /\ audit_stage = "P2Draining"
    /\ audit_stage' = "PreExit"
    /\ audit_pending' = NonTerminalInputs
    /\ audit_pre_verdict' = NoVerdict
    /\ audit_post_verdict' = NoVerdict
    /\ audit_abandoned' = {}
    /\ UNCHANGED vars
    /\ UNCHANGED <<setupVars, historyVars, restartVars>>

\* Before the exit: the inputs answered Abandon get the runless carrier.
AuditClassifyForCarriers ==
    /\ audit_stage = "PreExit"
    /\ audit_pending # {}
    /\ LET i == FirstOf(audit_pending) IN
          /\ Classify(i, audit_pre_verdict)
          /\ audit_pending' = audit_pending \ {i}
          /\ audit_carriers' = IF audit_pre_verdict'[i] = "Abandon"
                                  THEN audit_carriers \cup {i} ELSE audit_carriers
    /\ UNCHANGED <<audit_stage, setupVars, audit_post_verdict, audit_abandoned,
                   audit_kept, audit_stopped, restartVars>>

AuditStopExit ==
    /\ audit_stage = "PreExit"
    /\ audit_pending = {}
    /\ RuntimeExecutorExitedFromIdle
    /\ audit_stage' = "PostExit"
    /\ audit_pending' = NonTerminalInputs
    /\ UNCHANGED <<setupVars, audit_pre_verdict, audit_post_verdict, audit_abandoned,
                   historyVars, restartVars>>

\* After the exit: the cleanup classifies again and ends the Abandon set.
AuditClassifyForCleanup ==
    /\ audit_stage = "PostExit"
    /\ audit_pending # {}
    /\ LET i == FirstOf(audit_pending) IN
          /\ Classify(i, audit_post_verdict)
          /\ audit_pending' = audit_pending \ {i}
    /\ UNCHANGED <<audit_stage, setupVars, audit_pre_verdict, audit_abandoned,
                   historyVars, restartVars>>

ToEnd == Answered(audit_post_verdict, "Abandon") \ audit_abandoned

AuditStopAbandon ==
    /\ audit_stage = "PostExit"
    /\ audit_pending = {}
    /\ ToEnd # {}
    /\ LET i == FirstOf(ToEnd) IN
          /\ AbandonAny(i, "Stopped", Attempts(i))
          /\ audit_abandoned' = audit_abandoned \cup {i}
          /\ audit_stopped' = audit_stopped \cup {i}
    /\ UNCHANGED <<audit_stage, setupVars, audit_pending, audit_pre_verdict,
                   audit_post_verdict, audit_kept, audit_carriers, restartVars>>

AuditCommitStop ==
    /\ audit_stage = "PostExit"
    /\ audit_pending = {}
    /\ ToEnd = {}
    /\ audit_stage' = "P2Stopped"
    /\ audit_kept' = Answered(audit_post_verdict, "Keep")
    /\ UNCHANGED vars
    /\ UNCHANGED <<setupVars, regionVars, audit_carriers, audit_stopped, restartVars>>

\* The unregister drain may complete before the next process starts.
AuditUnregisterDrain ==
    /\ audit_stage = "P2Stopped"
    /\ \/ RuntimeLoopStoppedForUnregisterStopped(AuditSession, FALSE)
       \/ CommsDrainExitedForUnregisterStopped(AuditSession, FALSE)
       \/ CompletionWaitersResolvedForUnregisterStopped(AuditSession)
       \/ UnregisterSessionStopped(AuditSession, None, None, None, None)
    /\ AuditUnchanged

\* ---- process 3: a later start ----

AuditRestartProcess3 ==
    /\ audit_stage = "P2Stopped"
    /\ Restart("P3Start")
    /\ UNCHANGED <<setupVars, regionVars, historyVars, audit_recovered_p2, audit_staged_p3>>

AuditServe ==
    /\ audit_stage = "P3Serve"
    /\ \/ EnsureSessionWithExecutorIdle(AuditSession) /\ AuditUnchanged
       \/ PrepareAttached(AuditSession, AuditRun) /\ AuditUnchanged
       \/ \E i \in AuditInputs :
             /\ StageForRunRunning(i, AuditRun)
             /\ audit_staged_p3' = audit_staged_p3 \cup {i}
             /\ UNCHANGED <<audit_stage, setupVars, regionVars, historyVars, audit_rows,
                            audit_recovery, audit_recovered_p2>>

AuditNext ==
    \/ AuditSetup
    \/ AuditCrashProcess1
    \/ AuditStartProcess2
    \/ AuditRecoverRow
    \/ AuditRecoveryDone
    \/ AuditAdmitInProcess2
    \/ AuditBeginRollback
    \/ AuditOpenStop
    \/ AuditClassifyForCarriers
    \/ AuditStopExit
    \/ AuditClassifyForCleanup
    \/ AuditStopAbandon
    \/ AuditCommitStop
    \/ AuditUnregisterDrain
    \/ AuditRestartProcess3
    \/ AuditStartProcess3
    \/ AuditServe

AuditInit ==
    /\ Init
    /\ audit_stage = "Setup"
    /\ audit_facts = (AuditA :> [kind |-> "Prompt", durability |-> "Durable"])
                     @@ (AuditC :> [kind |-> "Prompt", durability |-> "Durable"])
    /\ audit_disposition = "None"
    /\ audit_c_planned = FALSE
    /\ audit_admitted_p2 = {}
    /\ audit_pending = {}
    /\ audit_pre_verdict = NoVerdict
    /\ audit_post_verdict = NoVerdict
    /\ audit_abandoned = {}
    /\ audit_kept = {}
    /\ audit_carriers = {}
    /\ audit_stopped = {}
    /\ audit_rows = [x \in {} |-> 0]
    /\ audit_recovery = [x \in {} |-> "Durability"]
    /\ audit_recovered_p2 = {}
    /\ audit_staged_p3 = {}

AuditSpec == AuditInit /\ [][AuditNext]_<<vars, auditVars>>

AuditStateConstraint == model_step_count <= AuditMaxSteps

\* ---- properties ----

StopStages == {"PreExit", "PostExit"}
OpenAnswers(v) == Answered(audit_pre_verdict, v) \cup Answered(audit_post_verdict, v)

AuditRollbackVerdictIsAFunction ==
    audit_stage \in StopStages =>
        \A i \in audit_pending :
            LET d == audit_facts[i].durability
                r == IF ENABLED RetainArm(i, d, FALSE, FALSE) THEN 1 ELSE 0
                k == IF ENABLED KeepArm(i, d, FALSE, FALSE) THEN 1 ELSE 0
                a == IF ENABLED AbandonArm(i, d, FALSE, FALSE) THEN 1 ELSE 0
            IN r + k + a = 1

AuditKeepEndsNoRecoveredInput ==
    audit_disposition = "Keep" =>
        /\ audit_recovered_p2 \cap OpenAnswers("Abandon") = {}
        /\ audit_recovered_p2 \cap audit_carriers = {}
        /\ audit_recovered_p2 \cap audit_stopped = {}

AuditKeepStopsWhatItAdmitted ==
    /\ audit_admitted_p2 \cap (OpenAnswers("Keep") \cup audit_kept) = {}
    /\ audit_admitted_p2 \cap audit_staged_p3 = {}

AuditKeepOnlyUnderKeep ==
    OpenAnswers("Keep") \cup audit_kept # {} => audit_disposition = "Keep"

AuditKeptInputSurvivesTheRollback ==
    audit_stage = "P2Stopped" => audit_kept \subseteq NonTerminalInputs

\* ---- reachability goals, each checked as an expected violation ----

\* B, queued behind process 1's run and recovered by process 2, is kept by
\* the Keep rollback and runs in process 3.
AuditWitnessKeptQueuedInputRuns ==
    /\ audit_disposition = "Keep"
    /\ AuditB \in audit_kept
    /\ AuditB \in audit_staged_p3
\* A, staged into process 1's run and queued again by process 2's recovery,
\* is kept by the Keep rollback and runs in process 3.
AuditWitnessKeptRequeuedInputRuns ==
    /\ audit_disposition = "Keep"
    /\ AuditA \in audit_kept
    /\ AuditA \in audit_staged_p3
\* C, admitted while process 2's registration stood, gets its carrier and
\* ends Stopped under Keep.
AuditWitnessAdmittedInputStoppedUnderKeep ==
    /\ audit_disposition = "Keep"
    /\ AuditC \in audit_stopped
    /\ AuditC \in audit_carriers
\* The control: a Stop rollback ends the recovered B.
AuditWitnessStopRollbackEndsRecoveredInput ==
    /\ audit_disposition = "Stop"
    /\ AuditB \in audit_stopped

NotAuditWitnessKeptQueuedInputRuns == ~AuditWitnessKeptQueuedInputRuns
NotAuditWitnessKeptRequeuedInputRuns == ~AuditWitnessKeptRequeuedInputRuns
NotAuditWitnessAdmittedInputStoppedUnderKeep == ~AuditWitnessAdmittedInputStoppedUnderKeep
NotAuditWitnessStopRollbackEndsRecoveredInput == ~AuditWitnessStopRollbackEndsRecoveredInput
====
