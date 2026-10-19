---- MODULE generation_highwater_audit ----
\* Hand-written bounded audit of the member generation high-water.
\*
\* Resume seeds the MobMachine from the events after the last MobReset only,
\* so it first recovers each identity's highest generation in the whole log
\* into identity_generation_highwater (RecoverIdentityGenerationHighwater),
\* then closes that window (SealIdentityGenerationHighwater). Minting (every
\* BeginSpawnExec* and CommitSpawnMembership* guard, and
\* ComputeRespawnGeneration) reads mob_machine_next_minted_generation: one
\* past the larger of the retained generation and the high-water. The epoch
\* replay accepts any ascending generation (at or above
\* mob_machine_next_respawn_generation of the retained map): an identity's
\* first generation there can continue its pre-reset history, and a mint
\* above the high-water can follow a generation an earlier restart reused.
\*
\* This audit keeps the generated MobMachine model unchanged and explores,
\* for one identity, two runtime ids, two fence tokens, one profile and
\* generations 0..2: the high-water recovery at every generation, the seal,
\* the roster recovery and the member reset recovery of the epoch replay,
\* in every order. Properties:
\*
\*   next_generation_above_highwater (generated invariant, kept from ci.cfg):
\*     whatever an identity mints next is above its recovered high-water.
\*   AuditSealClosesRecovery (action property): once sealed, the high-water
\*     never changes.
\*   AuditHighwaterNeverLowers (action property): no step lowers or drops a
\*     recorded high-water.
\*
\* AuditWitness* operators are reachability goals the runner checks as
\* expected violations, so the properties are not vacuous. With --mutants the
\* runner also checks that three seeded model defects are refused: a mint
\* helper that ignores the high-water, a recovery without its seal guard,
\* and a recovery without its not-lowered guard. A fourth mutant restores
\* the exact-successor replay rule and must make the two replay witnesses
\* unreachable, so the replay accepts those histories only because it
\* accepts an ascending generation.
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* generation_highwater_audit.sh. Run a deeper bound by hand with:
\*   specs/machines/mob_machine/generation_highwater_audit.sh 6
EXTENDS model

CONSTANT AuditMaxSteps

VARIABLES
    audit_raised,             \* a recovery re-recorded or raised an existing high-water
    audit_first_above_zero,   \* the replay's first generation for the identity was above 0
    audit_reset_gap           \* a replayed member reset skipped a generation

auditVars == <<audit_raised, audit_first_above_zero, audit_reset_gap>>

AuditIdentity == "agentidentity_1"

HighwaterRecover(g) == RecoverIdentityGenerationHighwaterRunning(AuditIdentity, g)

RosterRecover ==
    \E rid \in AgentRuntimeIdValues, fence \in FenceTokenValues, g \in GenerationValues,
       profile \in StringValues, mode \in SpawnPolicyRuntimeModeValues, external \in BOOLEAN :
        /\ RecoverRosterMemberRunning(AuditIdentity, rid, fence, g, profile, mode, external)
        /\ audit_first_above_zero' =
               (audit_first_above_zero
                \/ (AuditIdentity \notin DOMAIN identity_runtime_generations /\ g > 0))
        /\ UNCHANGED <<audit_raised, audit_reset_gap>>

RosterResetRecover ==
    \E previous \in AgentRuntimeIdValues, previous_g \in GenerationValues,
       rid \in AgentRuntimeIdValues, fence \in FenceTokenValues, g \in GenerationValues :
        /\ RecoverRosterMemberResetRunning(AuditIdentity, previous, previous_g, rid, fence, g)
        /\ audit_reset_gap' = (audit_reset_gap \/ g > previous_g + 1)
        /\ UNCHANGED <<audit_raised, audit_first_above_zero>>

AuditNext ==
    \/ \E g \in GenerationValues :
        /\ HighwaterRecover(g)
        /\ audit_raised' = (audit_raised \/ AuditIdentity \in DOMAIN identity_generation_highwater)
        /\ UNCHANGED <<audit_first_above_zero, audit_reset_gap>>
    \/ SealIdentityGenerationHighwaterRunning /\ UNCHANGED auditVars
    \/ RosterRecover
    \/ RosterResetRecover

AuditInit ==
    /\ Init
    /\ audit_raised = FALSE
    /\ audit_first_above_zero = FALSE
    /\ audit_reset_gap = FALSE

AuditSpec == AuditInit /\ [][AuditNext]_<<vars, auditVars>>

AuditStateConstraint == model_step_count <= AuditMaxSteps

AuditSealClosesRecovery ==
    [][identity_generation_highwater_sealed =>
         identity_generation_highwater' = identity_generation_highwater
      ]_<<vars, auditVars>>

AuditHighwaterNeverLowers ==
    [][\A id \in DOMAIN identity_generation_highwater :
         /\ id \in DOMAIN identity_generation_highwater'
         /\ identity_generation_highwater'[id] >= identity_generation_highwater[id]
      ]_<<vars, auditVars>>

\* Reachability goals, each checked as an expected violation of its negation.
\* A high-water is recorded.
AuditWitnessRecorded == AuditIdentity \in DOMAIN identity_generation_highwater
\* A recovery re-recorded or raised a recorded high-water.
AuditWitnessRaised == audit_raised
\* The window closed with a high-water recorded.
AuditWitnessSealedWithHighwater ==
    /\ identity_generation_highwater_sealed
    /\ AuditIdentity \in DOMAIN identity_generation_highwater
\* The epoch replay left the identity below its high-water, as a store a
\* pre-0.9.4 restart left: the invariant must hold there too.
AuditWitnessRetainedBelowHighwater ==
    /\ AuditIdentity \in DOMAIN identity_generation_highwater
    /\ AuditIdentity \in DOMAIN identity_runtime_generations
    /\ identity_runtime_generations[AuditIdentity] < identity_generation_highwater[AuditIdentity]
\* The replay's first generation for the identity is above zero: it continues
\* the identity's pre-reset history.
AuditWitnessFirstRecoveredAboveZero == audit_first_above_zero
\* A replayed member reset skipped a generation: the mint above the
\* high-water after a generation an earlier restart reused.
AuditWitnessResetGap == audit_reset_gap

NotAuditWitnessRecorded == ~AuditWitnessRecorded
NotAuditWitnessRaised == ~AuditWitnessRaised
NotAuditWitnessSealedWithHighwater == ~AuditWitnessSealedWithHighwater
NotAuditWitnessRetainedBelowHighwater == ~AuditWitnessRetainedBelowHighwater
NotAuditWitnessFirstRecoveredAboveZero == ~AuditWitnessFirstRecoveredAboveZero
NotAuditWitnessResetGap == ~AuditWitnessResetGap
====
