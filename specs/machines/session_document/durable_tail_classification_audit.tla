---- MODULE durable_tail_classification_audit ----
\* Hand-written audit of the durable-tail classification arms of the
\* generated SessionDocumentMachine model.
\*
\* Recovery classifies a session's durable tail from six observations: the
\* store's durable head relation, the run-id cardinality of the tail, its
\* terminal stop reason, its dangling tool-use and orphan tool-result counts,
\* and whether messages follow the terminal. ClassifyDurableTailCompleted and
\* ClassifyDurableTailRepairable need a verified descendant head (strict, or a
\* store-proven rewrite descendant) and exactly one run id;
\* ClassifyDurableTailAmbiguous is their exact complement. The DSL declares
\* state invariants only, and the generated Next is a disjunction, so
\* machine-verify never checks that the arms partition the observations. This
\* audit checks the generated TLA guards, independently of the Rust tables.
\*
\* The three arms are verdicts: each keeps phase Ready, changes only the step
\* count, and its guard reads no state but phase. The runner checks that
\* premise on the generated model before it runs TLC and fails if it stops
\* holding. So the initial state (phase Ready) stands for every reachable
\* state, and the audit checks it alone, with ENABLED over every tuple of the
\* observation product: 6 relations x 3 run-id cardinalities x 4 stop reasons
\* x dangling 0..2 x orphans 0..2 x messages after the terminal, 1296 tuples.
\* Invariants:
\*
\*   AuditClassificationIsTotalAndDisjoint (C1): exactly one arm is enabled
\*     for every tuple.
\*   AuditRewriteClassifiesLikeStrict (C2): the rewrite relation enables
\*     exactly the arm the strict relation enables, for every other value.
\*   AuditRewriteWithoutOneRunIdHolds (C3): the rewrite relation with no run
\*     id, or several, enables only Ambiguous, for every other value.
\*   AuditRewriteEndTurnCompletes (C4): the coherent single-run rewrite tail
\*     that ends at EndTurn (no dangling tool use, no orphan result, nothing
\*     after the terminal) enables exactly Completed.
\*   AuditRewriteToolUseRepairs: the same tail ending at ToolUse enables
\*     exactly Repairable, so every arm answers some rewrite tuple (C3 gives
\*     Ambiguous).
\*
\* AuditWitnessReady is the reachability goal the runner checks as an
\* expected violation: the initial state is Ready, so no invariant holds
\* vacuously. The runner's --mutants mode seeds defects into a copy of the
\* generated model and requires TLC to refuse each with the invariant it
\* targets, and records every invariant each defect breaks (TLC -continue)
\* together with the tuples AuditReportGaps names.
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* durable_tail_classification_audit.sh:
\*   specs/machines/session_document/durable_tail_classification_audit.sh --mutants
EXTENDS model

AuditSession == CHOOSE s \in SessionIdValues : TRUE
AuditCandidate == CHOOSE c \in RecoveryCandidateIdValues : TRUE
AuditCounts == 0..2
AuditArms == {"Completed", "Repairable", "Ambiguous"}

\* The arms whose generated action is enabled for one observation tuple.
ClassArms(rel, card, stop, d, o, after) ==
    { arm \in AuditArms :
        IF arm = "Completed"
        THEN ENABLED ClassifyDurableTailCompleted(AuditSession, AuditCandidate, rel, card, stop, d, o, after)
        ELSE IF arm = "Repairable"
        THEN ENABLED ClassifyDurableTailRepairable(AuditSession, AuditCandidate, rel, card, stop, d, o, after)
        ELSE ENABLED ClassifyDurableTailAmbiguous(AuditSession, AuditCandidate, rel, card, stop, d, o, after) }

Tuples ==
    DurableHeadRelationValues \X RunIdCardinalityValues \X DurableTailStopReasonValues
        \X AuditCounts \X AuditCounts \X BOOLEAN

Arms(t) == ClassArms(t[1], t[2], t[3], t[4], t[5], t[6])

AuditClassificationIsTotalAndDisjoint ==
    phase = "Ready" => \A t \in Tuples : Cardinality(Arms(t)) = 1

AuditRewriteClassifiesLikeStrict ==
    phase = "Ready" =>
        \A card \in RunIdCardinalityValues, stop \in DurableTailStopReasonValues,
           d \in AuditCounts, o \in AuditCounts, after \in BOOLEAN :
            ClassArms("VerifiedRewriteDescendant", card, stop, d, o, after)
                = ClassArms("VerifiedStrictDescendant", card, stop, d, o, after)

AuditRewriteWithoutOneRunIdHolds ==
    phase = "Ready" =>
        \A card \in RunIdCardinalityValues \ {"SingleRunId"},
           stop \in DurableTailStopReasonValues,
           d \in AuditCounts, o \in AuditCounts, after \in BOOLEAN :
            ClassArms("VerifiedRewriteDescendant", card, stop, d, o, after) = {"Ambiguous"}

AuditRewriteEndTurnCompletes ==
    phase = "Ready" =>
        ClassArms("VerifiedRewriteDescendant", "SingleRunId", "EndTurn", 0, 0, FALSE) = {"Completed"}

AuditRewriteToolUseRepairs ==
    phase = "Ready" =>
        ClassArms("VerifiedRewriteDescendant", "SingleRunId", "ToolUse", 0, 0, FALSE) = {"Repairable"}

\* Mutant runs only: names how many tuples enable no arm or several, with one
\* example of each, so a defect's gap is visible in its log. Always true.
AuditReportGaps ==
    LET none == {t \in Tuples : Arms(t) = {}}
        many == {t \in Tuples : Cardinality(Arms(t)) > 1}
    IN /\ PrintT(<<"tuples with no arm", Cardinality(none),
                   IF none = {} THEN "none" ELSE CHOOSE t \in none : TRUE>>)
       /\ PrintT(<<"tuples with several arms", Cardinality(many),
                   IF many = {} THEN "none" ELSE CHOOSE t \in many : TRUE>>)

\* The audit checks the initial state alone; the premise above makes it
\* stand for every reachable state.
AuditSpec == Init /\ [][FALSE]_vars

\* The reachability goal, checked as an expected violation.
AuditWitnessReady == phase = "Ready"
NotAuditWitnessReady == ~AuditWitnessReady
====
