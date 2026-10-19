---- MODULE member_lifecycle_audit ----
\* Hand-written bounded audit of the MobMachine member lifecycle.
\*
\* No generated profile model-checks MobMachine's member transitions to the
\* end: the ci profile has no identities (AgentIdentityValues = {}), and
\* deep.cfg, which has three, does not finish within the lane's per-run cap,
\* because the spawn arms (BeginSpawnExec*) bind 17 to 27 parameters, 12 of
\* them BOOLEAN, so a state with an enabled spawn has millions of
\* successors. This audit keeps the generated model unchanged and drives the
\* member lifecycle through the arms that do not spawn. A member enters
\* through the resume path's roster recovery (RecoverRosterMemberRunning) and
\* its session binding recovery, then takes progress observations, the local
\* Retire arms (releasing the session, preserving the binding, without a
\* binding; in Running and Stopped), the RetireMember signal, the session
\* ingress detach, and the runtime retirement and archive observations (in
\* Running and, where the machine has one, in Stopped). A later roster
\* recovery mints the next incarnation
\* of the same identity, so the first incarnation's runtime id, fence and
\* session become stale. Domains: one identity, two runtime ids, two fence
\* tokens, generations 0..1, two sessions, observation times 0, 60000 and
\* 300000 (the degraded and wedged thresholds), two progress tokens.
\*
\* Properties:
\*   every generated invariant of ci.cfg (kept);
\*   AuditObservationMonotonic (action property): while an identity stays
\*     recorded, its member_last_observed_at_ms never decreases. The guards
\*     that keep it so order optional values, which TLC evaluates only
\*     through the generated OptionLt / OptionLe: a renderer regression
\*     there fails this audit when TLC evaluates them.
\*
\* The runner requires every action it lists (covered_actions) to fire (TLC
\* coverage), a floor of distinct states at its bound, and every
\* AuditWitness* goal to be reached (as an expected violation of its
\* negation): a member bound to a session, a bound member's observation
\* time advancing, a Retiring member, an archived incarnation, and the
\* recovery of an archived identity's next incarnation. With --mutants it drops one observation_monotonic guard
\* and requires AuditObservationMonotonic to fail, then renders the same
\* guard as a plain `<=` between option records and requires TLC to refuse
\* to evaluate it.
\*
\* Not covered: states reachable only through BeginSpawnExec* (spawn phases,
\* spawn failures, provisioning), placed members and host bindings
\* (ObserveMemberRetirementArchivedPlaced*), remote and peer-only members
\* (RetireRemote*, RetireMemberRemote*, RetireMemberPeerOnly),
\* topology-preserving retirement (ObserveMemberRetirementArchived*
\* PreservingTopology; no abandoned respawn topology is reachable here),
\* flows and adaptive runs, and ObserveMemberRetirementArchivedStaleRuntime
\* and its Stopped twin: they need a Retiring runtime that its identity no
\* longer maps to, and here only the archive arms unmap an identity,
\* clearing its Retiring marker in the same step (the replay recovery arms
\* reach it).
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* member_lifecycle_audit.sh. Run a deeper bound by hand with:
\*   specs/machines/mob_machine/member_lifecycle_audit.sh 8
EXTENDS model

CONSTANT AuditMaxSteps

AuditIdentity == "agentidentity_1"
AuditRuntimes == {"agentruntimeid_1", "agentruntimeid_2"}
AuditFences == {1, 2}
AuditGenerations == {0, 1}
AuditSessions == {"sessionid_1", "sessionid_2"}
\* 60000 and 300000 are the generated degraded and wedged thresholds.
AuditTimes == {0, 60000, 300000}
AuditTokens == {"token_1", "token_2"}
\* No guard reads in_flight_work; the arms only record it.
AuditInFlight == {1}
AuditMob == "mobid_1"
AuditProfile == "profile_1"
AuditMode == "TurnDriven"
AuditDisposal == "Archived"

AuditRecoverMember ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations :
        RecoverRosterMemberRunning(AuditIdentity, rid, fence, g, AuditProfile, AuditMode, FALSE)

AuditBindSession ==
    \E rid \in AuditRuntimes, s \in AuditSessions :
        RecoverMemberSessionBindingFreshRunning(AuditIdentity, rid, s, None)

AuditProgressChangedOpen ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressChangedOpenRunning(AuditIdentity, TRUE, w, tok, t)

AuditProgressChangedIdle ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressChangedIdleRunning(AuditIdentity, FALSE, w, tok, t)

AuditProgressUnchangedIdle ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressUnchangedIdleRunning(AuditIdentity, FALSE, w, tok, t)

AuditProgressUnchangedOpenHealthy ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressUnchangedOpenHealthyRunning(AuditIdentity, TRUE, w, tok, t)

AuditProgressUnchangedOpenDegraded ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressUnchangedOpenDegradedRunning(AuditIdentity, TRUE, w, tok, t)

AuditProgressUnchangedOpenWedged ==
    \E w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressUnchangedOpenWedgedRunning(AuditIdentity, TRUE, w, tok, t)

AuditProgressStale ==
    \E open \in BOOLEAN, w \in AuditInFlight, tok \in AuditTokens, t \in AuditTimes :
        ObserveMemberProgressStaleRunning(AuditIdentity, open, w, tok, t)

AuditRetireReleasing ==
    \E rid \in AuditRuntimes, g \in AuditGenerations, s \in AuditSessions :
        RetireRunningReleasing(AuditMob, rid, AuditIdentity, g, Some(s), Some(s))

AuditRetirePreservingBinding ==
    \E rid \in AuditRuntimes, g \in AuditGenerations, s \in AuditSessions :
        RetireRunningPreservingBinding(AuditMob, rid, AuditIdentity, g, None, Some(s))

AuditRetireNoBinding ==
    \E rid \in AuditRuntimes, g \in AuditGenerations :
        RetireRunningNoBinding(AuditMob, rid, AuditIdentity, g, None, None)

AuditRetireStoppedReleasing ==
    \E rid \in AuditRuntimes, g \in AuditGenerations, s \in AuditSessions :
        RetireStoppedReleasing(AuditMob, rid, AuditIdentity, g, Some(s), Some(s))

AuditRetireStoppedPreservingBinding ==
    \E rid \in AuditRuntimes, g \in AuditGenerations, s \in AuditSessions :
        RetireStoppedPreservingBinding(AuditMob, rid, AuditIdentity, g, None, Some(s))

AuditRetireStoppedNoBinding ==
    \E rid \in AuditRuntimes, g \in AuditGenerations :
        RetireStoppedNoBinding(AuditMob, rid, AuditIdentity, g, None, None)

AuditRetireAbsent == RetireAbsentRunning(AuditIdentity)

AuditRetireAbsentStopped == RetireAbsentStopped(AuditIdentity)

AuditRetireMember ==
    \E rid \in AuditRuntimes, fence \in AuditFences, s \in AuditSessions :
        RetireMember(AuditIdentity, rid, fence, Some(s))

AuditDetachSessionIngress ==
    \E rid \in AuditRuntimes : SessionIngressDetachedForMobDestroyRunning(AuditMob, rid)

AuditRuntimeRetired ==
    \E rid \in AuditRuntimes, fence \in AuditFences : ObserveRuntimeRetired(rid, fence)

AuditRuntimeRetiredStopped ==
    \E rid \in AuditRuntimes, fence \in AuditFences : ObserveRuntimeRetiredStopped(rid, fence)

AuditArchivedLive ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedLive(AuditIdentity, rid, fence, g, Some(s), AuditDisposal, FALSE)

AuditArchivedRetired ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedRetired(AuditIdentity, rid, fence, g, Some(s), AuditDisposal, FALSE)

AuditArchivedLiveStopped ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedLiveStopped(AuditIdentity, rid, fence, g, Some(s), AuditDisposal, FALSE)

AuditArchivedRetiredStopped ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedRetiredStopped(AuditIdentity, rid, fence, g, Some(s), AuditDisposal, FALSE)

AuditArchivedAlreadyCleared ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedAlreadyCleared(AuditIdentity, rid, fence, g, Some(s), AuditDisposal)

AuditArchivedStaleRuntimeAlreadyCleared ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedStaleRuntimeAlreadyCleared(AuditIdentity, rid, fence, g, Some(s), AuditDisposal)

AuditArchivedAlreadyClearedStopped ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedAlreadyClearedStopped(AuditIdentity, rid, fence, g, Some(s), AuditDisposal)

AuditArchivedStaleRuntimeAlreadyClearedStopped ==
    \E rid \in AuditRuntimes, fence \in AuditFences, g \in AuditGenerations, s \in AuditSessions :
        ObserveMemberRetirementArchivedStaleRuntimeAlreadyClearedStopped(AuditIdentity, rid, fence, g, Some(s), AuditDisposal)

AuditQuiesceForStop == BeginPlacedCompletionLifecycleQuiesceFreshStop("Stop")

AuditStop == StopRunning

AuditResume == ResumeStopped

AuditNext ==
    \/ AuditRecoverMember
    \/ AuditBindSession
    \/ AuditProgressChangedOpen
    \/ AuditProgressChangedIdle
    \/ AuditProgressUnchangedIdle
    \/ AuditProgressUnchangedOpenHealthy
    \/ AuditProgressUnchangedOpenDegraded
    \/ AuditProgressUnchangedOpenWedged
    \/ AuditProgressStale
    \/ AuditRetireReleasing
    \/ AuditRetirePreservingBinding
    \/ AuditRetireNoBinding
    \/ AuditRetireStoppedReleasing
    \/ AuditRetireStoppedPreservingBinding
    \/ AuditRetireStoppedNoBinding
    \/ AuditRetireAbsent
    \/ AuditRetireAbsentStopped
    \/ AuditRetireMember
    \/ AuditDetachSessionIngress
    \/ AuditRuntimeRetired
    \/ AuditRuntimeRetiredStopped
    \/ AuditArchivedLive
    \/ AuditArchivedRetired
    \/ AuditArchivedLiveStopped
    \/ AuditArchivedRetiredStopped
    \/ AuditArchivedAlreadyCleared
    \/ AuditArchivedStaleRuntimeAlreadyCleared
    \/ AuditArchivedAlreadyClearedStopped
    \/ AuditArchivedStaleRuntimeAlreadyClearedStopped
    \/ AuditQuiesceForStop
    \/ AuditStop
    \/ AuditResume

AuditSpec == Init /\ [][AuditNext]_vars

AuditStateConstraint == model_step_count <= AuditMaxSteps

AuditObservationMonotonic ==
    [][\A id \in DOMAIN member_last_observed_at_ms :
         id \in DOMAIN member_last_observed_at_ms' =>
             member_last_observed_at_ms'[id] >= member_last_observed_at_ms[id]
      ]_vars

\* Reachability goals, each checked as an expected violation of its negation:
\* a state goal as an invariant, a step goal as an action property.
\* A member is bound to a session.
AuditWitnessBound ==
    /\ AuditIdentity \in DOMAIN identity_to_runtime
    /\ AuditIdentity \in DOMAIN member_session_bindings
\* A member is retiring.
AuditWitnessRetiring ==
    \E rid \in DOMAIN member_state_markers : member_state_markers[rid] = "Retiring"
\* An incarnation was archived: the identity keeps its history but no mapping.
AuditWitnessArchived ==
    /\ AuditIdentity \in DOMAIN identity_runtime_generations
    /\ AuditIdentity \notin DOMAIN identity_to_runtime
\* A bound member's recorded observation time advances: the progress guards
\* order Some(earlier) before Some(later) and admit the observation.
AuditWitnessObservationAdvancesStep ==
    /\ AuditWitnessBound
    /\ AuditIdentity \in DOMAIN member_last_observed_at_ms
    /\ AuditIdentity \in DOMAIN member_last_observed_at_ms'
    /\ member_last_observed_at_ms'[AuditIdentity] > member_last_observed_at_ms[AuditIdentity]
\* An archived identity's next incarnation is recovered at a higher
\* generation.
AuditWitnessSuccessorStep ==
    /\ AuditWitnessArchived
    /\ AuditIdentity \in DOMAIN identity_to_runtime'
    /\ AuditIdentity \in DOMAIN identity_runtime_generations'
    /\ identity_runtime_generations'[AuditIdentity] > identity_runtime_generations[AuditIdentity]

NotAuditWitnessBound == ~AuditWitnessBound
NotAuditWitnessRetiring == ~AuditWitnessRetiring
NotAuditWitnessArchived == ~AuditWitnessArchived
NotAuditWitnessObservationAdvances == [][~AuditWitnessObservationAdvancesStep]_vars
NotAuditWitnessSuccessor == [][~AuditWitnessSuccessorStep]_vars
====
