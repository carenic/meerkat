---- MODULE durable_tail_recovery_ownership_audit ----
\* Hand-written audit of the durable-tail recovery authorization arms of the
\* generated MeerkatMachine model, over the drive's store-proven ownership.
\*
\* A recovery drive asks AuthorizeDurableTailRecovery with seven
\* observations: the tail's class, the persisted lifecycle row, the run that
\* row names as current, the prior commit, the input evidence, and the
\* drive's ownership: NotProven, or the session's exclusive write authority,
\* proven by typed store facts, for a drive that hosts the session
\* (ExclusiveHost) or for a store-only write (ExclusiveWriter). The DSL
\* declares state invariants only, and the generated Next is a disjunction,
\* so machine-verify never checks that the arms partition the observations
\* or that ownership changes only what it is meant to. This audit checks the
\* generated TLA guards, independently of the Rust tables.
\*
\* The arms are verdicts whose guards read no state but phase, the current
\* run and the terminal run; the runner checks that premise on the generated
\* model before it runs TLC and fails if it stops holding. A deterministic
\* walk reaches every combination the audit needs: phase Idle and Retired,
\* each with no terminal run, the audit candidate's run terminal, or another
\* run terminal (never a current run: a current run refuses as RunFacts
\* whatever the persisted facts are, and nothing here changes that). In each
\* state, ENABLED over every tuple of the observation product (4 classes x 5
\* lifecycles x 3 current runs x 4 prior commits x 3 input evidences x 3
\* ownerships, 2160 tuples) checks:
\*
\*   AuditRecoveryIsTotalAndDisjoint: exactly one arm answers every tuple.
\*   AuditTerminalizedCandidateRefuses: once the candidate's run is terminal,
\*     every tuple refuses on the run facts.
\*   AuditUnprovenOwnershipGatesAsBefore: without proven ownership, a
\*     non-quiescent or undecodable lifecycle row, or a row naming any current
\*     run, refuses on the persisted facts (the rule before ownership).
\*   AuditOwnershipNeverAdmitsAnotherRunOrAnUndecodableRow: in every role, a
\*     row naming another run, or one that does not decode, refuses.
\*   AuditHostRecoversAsARestartDoes: an ExclusiveHost drive answers exactly
\*     what an unproven drive answers over the same tuple with the earlier
\*     incarnation's row read as quiescent (NonQuiescent as Idle, its
\*     candidate run as no run): the class decides as at a restart.
\*   AuditWriterRecoversAsRetired: an ExclusiveWriter drive answers exactly
\*     what an unproven drive answers over the same tuple with a Retired
\*     lifecycle row and no run: the earlier runtime is treated as retired.
\*   AuditWriterNeverContinues: a store-only writer never continues a run.
\*   AuditEarlierIncarnationsRunOutcomes: the pinned outcomes for an earlier
\*     incarnation's open run (NonQuiescent, its candidate run current):
\*     a host continues a continuable run, a writer commits it as
\*     interrupted, commits a completed one and holds an ambiguous one, and
\*     an unproven drive refuses.
\*
\* The witnesses NotAuditWitness{Idle,Retired,Terminalized} are reachability
\* goals the runner checks as expected violations, so no invariant holds
\* vacuously. The runner's --mutants mode seeds defects into a copy of the
\* generated model and requires TLC to refuse each with the invariant it
\* targets, and records every invariant each defect breaks (TLC -continue)
\* together with the tuples AuditReportGaps names.
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* durable_tail_recovery_ownership_audit.sh:
\*   specs/machines/meerkat_machine/durable_tail_recovery_ownership_audit.sh --mutants
EXTENDS model

AuditSession == "session_1"
AuditCandidate == "candidate_1"
\* The candidate run every invariant asks about, and another run the walk
\* terminalizes, so a terminal run that is not the candidate's is covered.
AuditRun == "run_1"
AuditOtherRun == "run_2"
AuditMaxSteps == 3

AuditArms == {
    "Commit", "Repair", "CommitCompletedRetainInputs", "Hold",
    "RefusePriorCommit", "HoldInputEvidence", "Continue",
    "RepairContinuableRetired", "HoldContinuableRetired",
    "RefuseRunFacts", "RefusePersistedFacts"
}

\* Each arm in either quiescent phase, for one observation tuple of the audit
\* candidate. The last committed sequence only feeds an update, so one value
\* stands for all.
ArmAction(arm, c, l, r, p, e, o) ==
    CASE arm = "Commit" ->
            AuthorizeDurableTailRecoveryCommitIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryCommitRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "Repair" ->
            AuthorizeDurableTailRecoveryRepairIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryRepairRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "CommitCompletedRetainInputs" ->
            AuthorizeDurableTailRecoveryCommitCompletedRetainInputsIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryCommitCompletedRetainInputsRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "Hold" ->
            AuthorizeDurableTailRecoveryHoldIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryHoldRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "RefusePriorCommit" ->
            AuthorizeDurableTailRecoveryRefusePriorCommitIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryRefusePriorCommitRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "HoldInputEvidence" ->
            AuthorizeDurableTailRecoveryHoldInputEvidenceIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryHoldInputEvidenceRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "Continue" ->
            AuthorizeDurableTailRecoveryContinueIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "RepairContinuableRetired" ->
            AuthorizeDurableTailRecoveryRepairContinuableRetiredIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryRepairContinuableRetiredRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "HoldContinuableRetired" ->
            AuthorizeDurableTailRecoveryHoldContinuableRetiredIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryHoldContinuableRetiredRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] arm = "RefuseRunFacts" ->
            AuthorizeDurableTailRecoveryRefuseRunFactsIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryRefuseRunFactsRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
      [] OTHER ->
            AuthorizeDurableTailRecoveryRefusePersistedFactsIdle(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)
            \/ AuthorizeDurableTailRecoveryRefusePersistedFactsRetired(AuditSession, AuditCandidate, AuditRun, c, l, r, 0, p, e, o)

\* The arms whose generated action is enabled for one observation tuple.
RecoveryArms(c, l, r, p, e, o) == { arm \in AuditArms : ENABLED ArmAction(arm, c, l, r, p, e, o) }

Tuples ==
    DurableTailRecoveryClassValues \X DurableRecoveryObservedLifecycleValues
        \X DurableRecoveryObservedRunValues \X DurableRecoveryPriorCommitValues
        \X DurableRecoveryInputEvidenceValues \X DurableRecoveryOwnershipValues

Arms(t) == RecoveryArms(t[1], t[2], t[3], t[4], t[5], t[6])

\* The tuple an unproven drive reads where the earlier incarnation's row is
\* replaced by `lifecycle` and no current run.
Unproven(t, lifecycle) == <<t[1], lifecycle, "NoRun", t[4], t[5], "NotProven">>

\* A restart reads a dead incarnation's row as quiescent.
RestartLifecycle(lifecycle) == IF lifecycle = "NonQuiescent" THEN "Idle" ELSE lifecycle

AuditQuiescentPhase == phase \in {"Idle", "Retired"}

\* The in-process facts admit recovery of the audit candidate.
AuditInProcessAdmits ==
    /\ AuditQuiescentPhase
    /\ current_run_id = None
    /\ turn_terminal_run_id # Some(AuditRun)

\* The rows an ownership proof is about: an earlier incarnation's row that
\* still decodes and names no other run.
Admissible(t) == t[2] # "Undecodable" /\ t[3] # "OtherRun"

AuditRecoveryIsTotalAndDisjoint ==
    AuditQuiescentPhase => \A t \in Tuples : Cardinality(Arms(t)) = 1

AuditTerminalizedCandidateRefuses ==
    (AuditQuiescentPhase /\ turn_terminal_run_id = Some(AuditRun)) =>
        \A t \in Tuples : Arms(t) = {"RefuseRunFacts"}

AuditUnprovenOwnershipGatesAsBefore ==
    AuditInProcessAdmits =>
        \A t \in Tuples :
            (/\ t[6] = "NotProven"
             /\ (t[2] \in {"NonQuiescent", "Undecodable"} \/ t[3] # "NoRun"))
                => Arms(t) = {"RefusePersistedFacts"}

AuditOwnershipNeverAdmitsAnotherRunOrAnUndecodableRow ==
    AuditInProcessAdmits =>
        \A t \in Tuples : ~Admissible(t) => Arms(t) = {"RefusePersistedFacts"}

AuditHostRecoversAsARestartDoes ==
    AuditInProcessAdmits =>
        \A t \in Tuples :
            (t[6] = "ExclusiveHost" /\ Admissible(t))
                => Arms(t) = Arms(Unproven(t, RestartLifecycle(t[2])))

AuditWriterRecoversAsRetired ==
    AuditInProcessAdmits =>
        \A t \in Tuples :
            (t[6] = "ExclusiveWriter" /\ Admissible(t))
                => Arms(t) = Arms(Unproven(t, "Retired"))

AuditWriterNeverContinues ==
    AuditQuiescentPhase =>
        \A t \in Tuples : t[6] = "ExclusiveWriter" => "Continue" \notin Arms(t)

\* An earlier incarnation's open run: its lifecycle row is non-quiescent and
\* names the candidate run as current. The rest of the tuple admits recovery.
OpenRun(class, ownership) ==
    RecoveryArms(class, "NonQuiescent", "CandidateRun", "NoPriorCommit", "AllBoundOrInert", ownership)

AuditEarlierIncarnationsRunOutcomes ==
    (AuditInProcessAdmits /\ phase = "Idle") =>
        /\ OpenRun("ContinuableCandidate", "ExclusiveHost") = {"Continue"}
        /\ OpenRun("ContinuableCandidate", "ExclusiveWriter") = {"RepairContinuableRetired"}
        /\ OpenRun("ContinuableCandidate", "NotProven") = {"RefusePersistedFacts"}
        /\ OpenRun("InterruptedRepairableCandidate", "ExclusiveHost") = {"Repair"}
        /\ OpenRun("InterruptedRepairableCandidate", "ExclusiveWriter") = {"Repair"}
        /\ OpenRun("CompletedCandidate", "ExclusiveWriter") = {"Commit"}
        /\ OpenRun("Ambiguous", "ExclusiveHost") = {"Hold"}
        /\ OpenRun("Ambiguous", "ExclusiveWriter") = {"Hold"}

\* Mutant runs only: names how many tuples enable no arm or several, with one
\* example of each, so a defect's gap is visible in its log. Always true.
AuditReportGaps ==
    AuditQuiescentPhase =>
        LET counted == {<<t, Cardinality(Arms(t))>> : t \in Tuples}
            none == {v \in counted : v[2] = 0}
            many == {v \in counted : v[2] > 1}
        IN /\ PrintT(<<"tuples with no arm", phase, Cardinality(none),
                       IF none = {} THEN "none" ELSE (CHOOSE v \in none : TRUE)[1]>>)
           /\ PrintT(<<"tuples with several arms", phase, Cardinality(many),
                       IF many = {} THEN "none" ELSE (CHOOSE v \in many : TRUE)[1]>>)

\* The walk: initialize, then retire, or commit a completed tail of either run
\* (which makes that run terminal), within AuditMaxSteps steps.
AuditCommit(run) ==
    \/ AuthorizeDurableTailRecoveryCommitIdle(AuditSession, AuditCandidate, run, "CompletedCandidate", "Idle", "NoRun", 0, "NoPriorCommit", "AllBoundOrInert", "NotProven")
    \/ AuthorizeDurableTailRecoveryCommitRetired(AuditSession, AuditCandidate, run, "CompletedCandidate", "Idle", "NoRun", 0, "NoPriorCommit", "AllBoundOrInert", "NotProven")

AuditNext ==
    /\ model_step_count < AuditMaxSteps
    /\ \/ Initialize
       \/ RetireRequestedFromIdleUnbound(AuditSession)
       \/ AuditCommit(AuditRun)
       \/ AuditCommit(AuditOtherRun)

AuditSpec == Init /\ [][AuditNext]_vars

\* Replaces the generated ci constraint, whose step bound is lower than the
\* walk's.
AuditStateConstraint == model_step_count <= AuditMaxSteps

\* The reachability goals, checked as expected violations.
AuditWitnessIdle == phase = "Idle" /\ turn_terminal_run_id = None
AuditWitnessRetired == phase = "Retired" /\ turn_terminal_run_id = None
AuditWitnessTerminalized ==
    /\ phase = "Retired"
    /\ turn_terminal_run_id = Some(AuditRun)
AuditWitnessOtherTerminal ==
    /\ phase = "Idle"
    /\ turn_terminal_run_id = Some(AuditOtherRun)
NotAuditWitnessIdle == ~AuditWitnessIdle
NotAuditWitnessRetired == ~AuditWitnessRetired
NotAuditWitnessTerminalized == ~AuditWitnessTerminalized
NotAuditWitnessOtherTerminal == ~AuditWitnessOtherTerminal
====
