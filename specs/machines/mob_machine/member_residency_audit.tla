---- MODULE member_residency_audit ----
\* Hand-written audit of MobMachine member residency (Dormant, Waking, Live).
\*
\* The generated ci profile has no identities, so the residency arms and the
\* residency invariants (I1-I14) hold there only vacuously. This audit keeps
\* the generated model unchanged and drives the residency arms in eight runs,
\* each over a small, fixed domain, so every run fits the canonical lane's
\* per-run cap:
\*
\*   seed (AuditSeedSpec): one identity recovered with no, an inproc or a
\*     network endpoint, every build seed (restore policy, composition
\*     authority, host-pending set) in every lifecycle phase, every boot verdict
\*     (Wake, VerifyThenDormant, StayDormant), the seal and the host
\*     carry-out, with one authorized wake and its abandon.
\*   wake (AuditWakeSpec): one identity under the default seed: the boot plan
\*     and seal, every WakeMember arm for six input shapes, every
\*     attempt-fenced resolution, the late (StaleAttempt) feedback in Running
\*     and Stopped, the runtime-ready observation with and without an epoch,
\*     three release shapes (the recorded epoch, another epoch, no epoch), the
\*     drift and terminal classifications, the peer notice classification,
\*     the replayed restore failures, the release verdict, Stop and Resume.
\*   retire (AuditRetireSpec): one identity woken, released and retired: the
\*     routed and the archive-only retirement starts (R27), the routed retire
\*     retry, the runtime retire observation, the session ingress detach, the
\*     plain archives, the replayed retirement starts, a rotation with its
\*     terminal in Running and Stopped, a host-pending seed and its
\*     carry-out, and the mob's destroy: its admission, its member retirement
\*     and archive arms and DestroyMob (R124).
\*   terminal (AuditTerminalSpec): one identity in the Completed and
\*     Destroyed phases: Complete, MarkCompleted, the destroy, Shutdown, the
\*     late feedback, the release, the wake refusal, the notice
\*     classification and the release verdict there, and the replayed Retired
\*     record.
\*   resume (AuditResumeSpec): one identity through an explicit resume: Stop,
\*     the resume's preparation and Resume, the three member reservation arms
\*     (Dormant, Live, Join), the live classification, the settle arms
\*     (exact and replaced session), readiness, topology and finish, with the
\*     wake arms and resolutions it meets and a session replacement.
\*   edge (AuditEdgeSpec): two identities and one wiring edge (I8): both
\*     recovered, bound and given endpoints, the edge recovered or wired, the
\*     seal, one member retired and archived with and without its respawn
\*     topology preserved, and its next incarnation recovered.
\*   seat (AuditSeatSpec): two identities from the spawn ladder's Opened
\*     phase: the dormant seat with an endpoint, with another member's peer id
\*     (R5) or with none (EndpointPending), the embodied spawn commit, the
\*     runtime-ready observation and the release that clears the startup ladder
\*     (R12), the boot plan and seal, the endpoint registration, and the
\*     rotation (retirement start, intent, terminal over a live or a retired
\*     runtime) with and without an endpoint.
\*   wake2 (AuditWake2Spec): two identities, each woken, released and
\*     notified by the other: a peer notice between members, a wake while
\*     the sender is Waking, and a retirement of one member while the other is
\*     Waking.
\*
\* Domains are fixed per run (see the runner). Each run explores its whole
\* reachable space under a state constraint that caps the machine's wake
\* attempt mint (member_wake_attempt_sequence <= AuditMaxAttempts, set per run)
\* and a VIEW that leaves out model_step_count and topology_epoch, which no
\* guard reads.
\*
\* Properties, in every run:
\*   every generated invariant of ci.cfg (kept), I1-I14 among them;
\*   AuditAttemptEndsClosed (AP2, action property, safety form): an open wake
\*     attempt is never replaced, and it disappears only in a step that leaves
\*     its member Live, Dormant or without a phase (Broken, retiring, unseated);
\*   AuditWakingClosable (invariant, the ENABLED form of AP2): every
\*     Waking member has an enabled closing arm for every closing kind
\*     (Unsettled and ReadinessFailed abandons, Failed, HostRefused,
\*     Retryable), whatever its other feedback preconditions.
\* Per run also:
\*   AP1 (action property): no recovery or seat arm overwrites an existing
\*     residency phase (seed, wake, resume, edge, seat, terminal);
\*   seed: AuditPlanLeavesSealable (after its boot plan a member has a durable
\*     endpoint or owes EndpointPending, so the seal never refuses it);
\*   wake: AuditLifecycleNeverWakes (AP3), AuditWakeNeverSkipsWaking,
\*     AuditWakeExactlyOne (exactly one WakeMember arm answers every input
\*     shape), AuditSucceededLeavesEndpoint (a local wake never closes Live
\*     without an endpoint while it owes EndpointPending),
\*     AuditWakeClosesLiveUnlessEndpointOwed (a bound wake owing no endpoint
\*     cause can always close Live) and AuditReleaseEpochFence (a release
\*     whose epoch is not the member's recorded one never demotes it);
\*   retire: AuditRetireRoutesOnlyRegistered (R27: no runtime retire is routed
\*     for a member with no registration), AuditRetirementArchivable (R124: a
\*     retiring member whose ingress detach closed has an enabled archive
\*     terminal, and under an admitted destroy no rotation and no host
\*     carry-out holds it) and AuditDestroyFencesRotation (R124: no rotation
\*     begins or commits under an admitted destroy);
\*   seat: AuditSeatLeavesDormant (a dormant seat leaves its identity
\*     Dormant, with its endpoint or owed EndpointPending, and requests no
\*     runtime binding).
\* I5 is an ASSUME over the generated wake refusal helper's whole input domain:
\* a present, session-bound member with a phase is never refused as not a
\* member. A second ASSUME pins the release verdict helper: Release exactly for
\* a seated, local, Live member, WakeInFlight exactly for a Waking one.
\*
\* Not covered: I3 (a Dormant member is local) needs a placed member, which
\* only the placement arms create; the placed revival arm likewise. The machine
\* debug invariants and the meerkat-mob placed tests pin them.
\*
\* The TLC configs are DERIVED from the generated ci.cfg on every run by
\* member_residency_audit.sh.
EXTENDS model

\* The positions of model_step_count and topology_epoch in the generated
\* `vars` tuple; the runner reads them from model.tla.
CONSTANTS AuditStepIndex, AuditEpochIndex, AuditMaxAttempts

Id1 == "agentidentity_1"
Id2 == "agentidentity_2"
Rid1 == "agentruntimeid_1"
Rid2 == "agentruntimeid_2"
Rid3 == "agentruntimeid_3"
Sess1 == "sessionid_1"
Sess2 == "sessionid_2"
Sess3 == "sessionid_3"
AuditMob == "mobid_1"
AuditProfile == "profile_1"
AuditModel == "model_1"
AuditDigest == "digest_1"
AuditMode == "TurnDriven"
AuditReason == "reason_1"
AuditCode == "code_1"
AuditDisposal == "Archived"
AuditResumeAttempt == "resumeattemptid_1"
AuditObjective == "objective_1"
Epoch1 == Some("runtimeepochid_1")
Epoch2 == Some("runtimeepochid_2")
AuditEpochs == {None, Epoch1}
AuditAttempts == 1..AuditMaxAttempts
AuditInputClasses == {"Content", "Lifecycle"}
AuditWakeCauses == {"Boot", "HostDelivery", "PeerDelivery", "Operator"}

\* The wake run's input shapes <<cause, observed_missing, input_class,
\* sender_waking>>: a plain delivery, an observed miss (the drift path of a
\* Live member), a lifecycle notice (never wakes, AP3), a delivery from a
\* waking sender (SenderWaking), and the operator's and the boot's own wakes.
AuditWakeShapes ==
    { <<"HostDelivery", FALSE, "Content", FALSE>>,
      <<"HostDelivery", TRUE, "Content", FALSE>>,
      <<"PeerDelivery", FALSE, "Lifecycle", FALSE>>,
      <<"PeerDelivery", FALSE, "Content", TRUE>>,
      <<"Operator", FALSE, "Content", FALSE>>,
      <<"Boot", FALSE, "Content", FALSE>> }

\* Member 1's inproc and network endpoints share its peer id; member 2 and the
\* rotated incarnation have their own.
E1Inproc == [name |-> "peername_1", peer_id |-> "peerid_1", address |-> "inproc://peername_1",
             signing_key |-> "peersigningkey_1", transport |-> "Inproc"]
E1Tcp == [name |-> "peername_1", peer_id |-> "peerid_1", address |-> "tcp://peername_1",
          signing_key |-> "peersigningkey_1", transport |-> "Tcp"]
E2Inproc == [name |-> "peername_2", peer_id |-> "peerid_2", address |-> "inproc://peername_2",
             signing_key |-> "peersigningkey_2", transport |-> "Inproc"]
E3Inproc == [name |-> "peername_3", peer_id |-> "peerid_3", address |-> "inproc://peername_3",
             signing_key |-> "peersigningkey_3", transport |-> "Inproc"]

AuditEdge == [a |-> Id1, b |-> Id2]

AuditResumeBinding ==
    [agent_runtime_id |-> Rid1, fence_token |-> 1, session_id |-> Sess1, definition_epoch |-> 1]

(***************************************************************************)
(* Arm families shared by the runs.                                        *)
(***************************************************************************)

\* Every WakeMember arm, in every lifecycle phase.
WakeArm(id, cause, om, ic, sw) ==
    \/ WakeMemberAuthorizedRunning(id, cause, om, ic, sw)
    \/ WakeMemberJoinedRunning(id, cause, om, ic, sw)
    \/ WakeMemberAlreadyLiveRunning(id, cause, om, ic, sw)
    \/ WakeMemberRefusedRunning(id, cause, om, ic, sw)
    \/ WakeMemberRefusedStopped(id, cause, om, ic, sw)
    \/ WakeMemberRefusedCompleted(id, cause, om, ic, sw)
    \/ WakeMemberRefusedDestroyed(id, cause, om, ic, sw)

\* Every cause, observed miss, input class and sender phase (the resume run).
AuditWake(id) ==
    \E cause \in AuditWakeCauses, om \in BOOLEAN, ic \in AuditInputClasses, sw \in BOOLEAN :
        WakeArm(id, cause, om, ic, sw)

\* The wake run's input shapes.
AuditWakeShaped(id) ==
    \E s \in AuditWakeShapes : WakeArm(id, s[1], s[2], s[3], s[4])

\* One operator wake (the runs that only need a member woken).
AuditOperatorWake(id) == WakeArm(id, "Operator", FALSE, "Content", FALSE)

\* The attempt-fenced resolutions of a Running mob for one incarnation.
AuditResolve(id, rid, fence, g, sessions) ==
    \E a \in AuditAttempts :
        \/ ResolveMemberRevivalBoundRunning(id, a)
        \/ ResolveMemberRevivalSucceededRunningLocal(id, a)
        \/ ResolveMemberRevivalFailedRunning(id, AuditReason, a)
        \/ ResolveMemberRevivalHostRefusedRunning(id, AuditCode, AuditReason, a)
        \/ ResolveMemberRevivalRetryableRunning(id, a, "WakeTimedOut")
        \/ AbandonMemberWakeDormantRunning(id, a, "Unsettled", AuditReason)
        \/ AbandonMemberWakeBrokenRunning(id, a, "ReadinessFailed", AuditReason)
        \/ \E s \in sessions : ResolveRecreatedMemberSessionSucceededRunningLocal(id, rid, fence, g, s, a)

\* The success path of one wake and its plain abandon.
AuditWakePath(id) ==
    \E a \in AuditAttempts :
        \/ ResolveMemberRevivalBoundRunning(id, a)
        \/ ResolveMemberRevivalSucceededRunningLocal(id, a)
        \/ AbandonMemberWakeDormantRunning(id, a, "Unsettled", AuditReason)

\* Late feedback for the last minted attempt (stale once it closed): a typed
\* StaleAttempt no-op, in Running and Stopped. Each disjunct is a plain
\* application (no LET), so TLC coverage names it after its generated arm.
AuditStaleFeedback(id, rid, fence, g, s) ==
    \/ ResolveMemberRevivalBoundStaleRunning(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalBoundStaleStopped(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalSucceededStaleRunning(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalSucceededStaleStopped(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalFailedStaleRunning(id, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalFailedStaleStopped(id, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalHostRefusedStaleRunning(id, AuditCode, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalHostRefusedStaleStopped(id, AuditCode, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalRetryableStaleRunning(id, member_wake_attempt_sequence, "WakeTimedOut")
    \/ ResolveMemberRevivalRetryableStaleStopped(id, member_wake_attempt_sequence, "WakeTimedOut")
    \/ AbandonMemberWakeStaleRunning(id, member_wake_attempt_sequence, "Unsettled", AuditReason)
    \/ AbandonMemberWakeStaleStopped(id, member_wake_attempt_sequence, "Unsettled", AuditReason)
    \/ ResolveRecreatedMemberSessionSucceededStaleRunning(id, rid, fence, g, s, member_wake_attempt_sequence)
    \/ ResolveRecreatedMemberSessionSucceededStaleStopped(id, rid, fence, g, s, member_wake_attempt_sequence)

\* The same late feedback in the Completed and Destroyed phases.
AuditStaleFeedbackTerminal(id, rid, fence, g, s) ==
    \/ ResolveMemberRevivalBoundStaleCompleted(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalBoundStaleDestroyed(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalSucceededStaleCompleted(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalSucceededStaleDestroyed(id, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalFailedStaleCompleted(id, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalFailedStaleDestroyed(id, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalHostRefusedStaleCompleted(id, AuditCode, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalHostRefusedStaleDestroyed(id, AuditCode, AuditReason, member_wake_attempt_sequence)
    \/ ResolveMemberRevivalRetryableStaleCompleted(id, member_wake_attempt_sequence, "WakeTimedOut")
    \/ ResolveMemberRevivalRetryableStaleDestroyed(id, member_wake_attempt_sequence, "WakeTimedOut")
    \/ AbandonMemberWakeStaleCompleted(id, member_wake_attempt_sequence, "Unsettled", AuditReason)
    \/ AbandonMemberWakeStaleDestroyed(id, member_wake_attempt_sequence, "Unsettled", AuditReason)
    \/ ResolveRecreatedMemberSessionSucceededStaleCompleted(id, rid, fence, g, s, member_wake_attempt_sequence)
    \/ ResolveRecreatedMemberSessionSucceededStaleDestroyed(id, rid, fence, g, s, member_wake_attempt_sequence)

\* The release arms of one shape in the Running and Stopped phases.
ReleaseArm(rid, fence, ep) ==
    \/ ReleaseMemberMaterializationReleasedRunning(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationReleasedStopped(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleRunning(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleStopped(rid, fence, ep, "RegistrationReleased")

\* The release epochs of the wake run: the recorded one, another one (the
\* stale release after a newer wake) and none.
AuditReleaseEpochs == {Epoch1, Epoch2, None}

AuditSeed(hostPendingSets) ==
    \E policy \in {"OnDemand", "OnDemandVerified", "Eager"},
       authority \in {"Authoritative", "NonAuthoritative"},
       hp \in hostPendingSets :
        \/ SeedMemberResidencyRunning(policy, authority, hp)
        \/ SeedMemberResidencyStopped(policy, authority, hp)
        \/ SeedMemberResidencyCompleted(policy, authority, hp)

AuditPlan(id) ==
    \E ob \in {"None", "RuntimeWork"}, hr \in BOOLEAN, notified \in BOOLEAN :
        \/ PlanBootMaterializationWake(id, ob, hr, notified)
        \/ PlanBootMaterializationVerify(id, ob, hr, notified)
        \/ PlanBootMaterializationStayDormant(id, ob, hr, notified)

\* The boot plan of the default seed: an obligation or none.
AuditPlanDefault(id) ==
    \E ob \in {"None", "RuntimeWork"} :
        \/ PlanBootMaterializationWake(id, ob, FALSE, FALSE)
        \/ PlanBootMaterializationStayDormant(id, ob, FALSE, FALSE)

AuditHostPrepared(id) ==
    \/ ResolveMemberHostPreparationRunning(id)
    \/ ResolveMemberHostPreparationStopped(id)
    \/ ResolveMemberHostPreparationCompleted(id)

AuditNotice(id) ==
    \E ic \in AuditInputClasses :
        \/ ClassifyPeerNoticeRunning(id, id, ic)
        \/ ClassifyPeerNoticeStopped(id, id, ic)

\* The release verdict for one member in the Running and Stopped phases.
AuditReleaseVerdict(id) ==
    \/ ClassifyMemberReleaseRunning(id)
    \/ ClassifyMemberReleaseStopped(id)

AuditClassify(id) ==
    \E obs \in {"DurableSnapshotPresent", "DurableSnapshotMissing"} :
        \/ ClassifyMemberLiveMaterializationRevivable(id, obs, AuditReason)
        \/ ClassifyMemberLiveMaterializationTerminal(id, obs, AuditReason)
        \/ ClassifyMemberLiveMaterializationTerminalDormant(id, obs, AuditReason)

AuditQuiesceStop == BeginPlacedCompletionLifecycleQuiesceFreshStop("Stop")
AuditQuiesceComplete == BeginPlacedCompletionLifecycleQuiesceFresh("Complete")
AuditQuiesceDestroy == BeginPlacedCompletionLifecycleQuiesceFresh("Destroy")

\* Member 1 (Rid1, fence 1, generation 0, session 1): recovery and its seat.
WRecover == RecoverRosterMemberRunning(Id1, Rid1, 1, 0, AuditProfile, AuditMode, FALSE)
WBind == RecoverMemberSessionBindingFreshRunning(Id1, Rid1, Sess1, None)
WBindCurrent == RecoverMemberSessionBindingAlreadyCurrentRunning(Id1, Rid1, Sess1, None)
WEndpoint == RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, E1Inproc)
WRegisterPeer == RegisterMemberPeerRunning(Id1, Rid1, 0, 1, E1Inproc)
WRecoverRetired == \E p \in BOOLEAN, started \in BOOLEAN : RecoverRosterMemberRetiredRunning(Id1, Rid1, 0, p, started)
\* The replayed restore failures (Broken setters, R2).
WRestoreFailure ==
    \/ RecoverMemberRestoreFailureRunning(Id1, AuditReason)
    \/ RecoverMemberRestoreHostRefusedRunning(Id1, AuditCode, AuditReason)
WReady == \E ep \in AuditEpochs : ObserveRuntimeReady(Rid1, 1, ep)
WReadyEpoch1 == ObserveRuntimeReady(Rid1, 1, Epoch1)
WRelease == \E ep \in AuditReleaseEpochs : ReleaseArm(Some(Rid1), Some(1), ep)
WReleaseEpoch1 == ReleaseArm(Some(Rid1), Some(1), Epoch1)
WDetach ==
    \/ SessionIngressDetachedForMobDestroyRunning(AuditMob, Rid1)
    \/ SessionIngressDetachedForMobDestroyStopped(AuditMob, Rid1)

\* Member 1's phase as an option, before and after a step; likewise member 2.
Phase1 == IF Id1 \in DOMAIN member_materialization THEN Some(member_materialization[Id1]) ELSE None
Phase1Next == IF Id1 \in DOMAIN member_materialization' THEN Some(member_materialization'[Id1]) ELSE None
Phase2 == IF Id2 \in DOMAIN member_materialization THEN Some(member_materialization[Id2]) ELSE None

(***************************************************************************)
(* The seed run: the build seed, the boot plan, the seal and the host      *)
(* carry-out.                                                              *)
(***************************************************************************)

SEndpointAny == \E e \in {E1Inproc, E1Tcp} : RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, e)

SeedRecoverArms == WRecover \/ WBind \/ SEndpointAny

AuditSeedNext ==
    \/ WRecover
    \/ WBind
    \/ SEndpointAny
    \/ AuditSeed({{}, {Id1}})
    \/ AuditHostPrepared(Id1)
    \/ AuditPlan(Id1)
    \/ SealBootMaterializationPlan
    \/ AuditOperatorWake(Id1)
    \/ \E a \in AuditAttempts : AbandonMemberWakeDormantRunning(Id1, a, "Unsettled", AuditReason)
    \/ AuditQuiesceStop
    \/ StopRunning
    \/ ResumeStopped
    \/ AuditQuiesceComplete
    \/ CompleteRunning

AuditSeedSpec == Init /\ [][AuditSeedNext]_vars

(***************************************************************************)
(* The wake run: one identity, one incarnation, the default seed.          *)
(***************************************************************************)

\* The recovery and seat arms of the wake run (AP1).
WRecoverArms == WRecover \/ WBind \/ WBindCurrent \/ WEndpoint \/ WRestoreFailure

AuditWakeNext ==
    \/ WRecover
    \/ WBind
    \/ WBindCurrent
    \/ WEndpoint
    \/ WRegisterPeer
    \/ WRestoreFailure
    \/ AuditPlanDefault(Id1)
    \/ SealBootMaterializationPlan
    \/ AuditWakeShaped(Id1)
    \/ AuditResolve(Id1, Rid1, 1, 0, {Sess1})
    \/ AuditStaleFeedback(Id1, Rid1, 1, 0, Sess1)
    \/ WReady
    \/ WRelease
    \/ AuditClassify(Id1)
    \/ AuditNotice(Id1)
    \/ AuditReleaseVerdict(Id1)
    \/ AuditQuiesceStop
    \/ StopRunning
    \/ ResumeStopped

AuditWakeSpec == Init /\ [][AuditWakeNext]_vars

(***************************************************************************)
(* The retire run: one identity woken, released, retired, rotated and      *)
(* destroyed (R27, R124).                                                  *)
(***************************************************************************)

\* The routed retirement starts (a Live member) and their archive-only twins
\* (a Dormant or Waking member, or a re-drive of an archive-only one).
RetireArms ==
    \/ RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningReleasingArchiveOnly(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireRunningPreservingBindingArchiveOnly(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireStoppedReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireStoppedReleasingArchiveOnly(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireStoppedPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireStoppedPreservingBindingArchiveOnly(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireMember(Id1, Rid1, 1, Some(Sess1))
    \/ RetireMemberArchiveOnly(Id1, Rid1, 1, Some(Sess1))

\* Every arm that routes a runtime retire (R27: never for a member with no
\* registration).
RoutingRetire ==
    \/ RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireStoppedReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireStoppedPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireMember(Id1, Rid1, 1, Some(Sess1))
    \/ AdmitDestroyMemberRetireLiveRunning(AuditMob, Id1, Rid1, 1, 0, Some(Sess1))
    \/ RetryRuntimeRetireRunning(Id1, Rid1)
    \/ RetryRuntimeRetireStopped(Id1, Rid1)

RRetry == RetryRuntimeRetireRunning(Id1, Rid1) \/ RetryRuntimeRetireStopped(Id1, Rid1)
RRuntimeRetired == ObserveRuntimeRetired(Rid1, 1) \/ ObserveRuntimeRetiredStopped(Rid1, 1)

\* The plain archive terminals of member 1's retirement.
PlainArchives ==
    \/ ObserveMemberRetirementArchivedLive(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedRetired(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedLiveStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedRetiredStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)

\* The replayed retirement starts (archive-only: nothing is registered after
\* a restart).
RRecoverStarted ==
    \/ RecoverRosterMemberRetirementStartedReleasing(Id1, Rid1, 0, Some(Sess1), Some(Sess1), None, FALSE)
    \/ RecoverRosterMemberRetirementStartedPreservingBinding(Id1, Rid1, 0, None, Some(Sess1), None, FALSE)

\* The rotation onto session 3 (no endpoint), its next incarnation Rid3,
\* fence 3, generation 1.
RAuthorize ==
    AuthorizeSpawnProfileRunning(Id1, AuditProfile, AuditModel, AuditDigest, AuditDigest, AuditDigest, None, None, FALSE, None)
RBeginRotation == BeginMemberRotationRunning(Id1, Rid1, 1, 0, Sess3, None, FALSE)
RCommitRotation ==
    \/ CommitMemberRotationLiveRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
    \/ CommitMemberRotationRetiredRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
    \/ CommitMemberRotationLiveStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
    \/ CommitMemberRotationRetiredStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
RotationArms == RBeginRotation \/ RCommitRotation

\* The host marks member 1's preparation pending at build (a rotation lost in
\* a crash is re-driven by the host), or not.
RSeedHostPending == \E hp \in {{}, {Id1}} : SeedMemberResidencyRunning("OnDemand", "Authoritative", hp)

\* The mob's destroy: its member retirement admissions and archives.
RDestroyMember ==
    \/ AdmitDestroyMemberRetireLiveRunning(AuditMob, Id1, Rid1, 1, 0, Some(Sess1))
    \/ AdmitDestroyMemberRetireArchiveOnlyRunning(AuditMob, Id1, Rid1, 1, 0, Some(Sess1))
    \/ AdmitDestroyMemberRetireAlreadyRetiringSessionRunning(AuditMob, Id1, Rid1, 1, 0, Some(Sess1))
    \/ AdmitDestroyMemberRetireAlreadyRetiringReleasedSessionRunning(AuditMob, Id1, Rid1, 1, 0, Some(Sess1))
    \/ ObserveDestroyMemberRetirementArchivedLiveRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal)
    \/ ObserveDestroyMemberRetirementArchivedRetiredRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal)

AuditRetireNext ==
    \/ WRecover
    \/ WBind
    \/ WEndpoint
    \/ RSeedHostPending
    \/ ResolveMemberHostPreparationRunning(Id1)
    \/ AuditOperatorWake(Id1)
    \/ AuditWakePath(Id1)
    \/ WReadyEpoch1
    \/ WReleaseEpoch1
    \/ RetireArms
    \/ RRetry
    \/ RRuntimeRetired
    \/ WDetach
    \/ PlainArchives
    \/ RRecoverStarted
    \/ RAuthorize
    \/ RotationArms
    \/ AuditQuiesceStop
    \/ StopRunning
    \/ ResumeStopped
    \/ AuditQuiesceDestroy
    \/ AdmitDestroyCleanup
    \/ RDestroyMember
    \/ DestroyMob(Sess1)

AuditRetireSpec == Init /\ [][AuditRetireNext]_vars

(***************************************************************************)
(* The terminal run: the Completed and Destroyed phases.                   *)
(***************************************************************************)

TRelease ==
    \/ ReleaseMemberMaterializationReleasedCompleted(Some(Rid1), Some(1), Epoch1, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleCompleted(Some(Rid1), Some(1), Epoch1, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleDestroyed(Some(Rid1), Some(1), Epoch1, "RegistrationReleased")

TNotice ==
    \E ic \in AuditInputClasses :
        \/ ClassifyPeerNoticeCompleted(Id1, Id1, ic)
        \/ ClassifyPeerNoticeDestroyed(Id1, Id1, ic)

TRecoverArms == WRecover \/ WBind \/ WEndpoint \/ WRecoverRetired

AuditTerminalNext ==
    \/ WRecover
    \/ WBind
    \/ WEndpoint
    \/ WRecoverRetired
    \/ AuditOperatorWake(Id1)
    \/ AuditWakePath(Id1)
    \/ WReadyEpoch1
    \/ TRelease
    \/ TNotice
    \/ ClassifyMemberReleaseCompleted(Id1)
    \/ ClassifyMemberReleaseDestroyed(Id1)
    \/ AuditStaleFeedbackTerminal(Id1, Rid1, 1, 0, Sess1)
    \/ AuditQuiesceComplete
    \/ CompleteRunning
    \/ MarkCompleted
    \/ AuditQuiesceDestroy
    \/ AdmitDestroyCleanup
    \/ DestroyMob(Sess1)
    \/ ShutdownRunning

AuditTerminalSpec == Init /\ [][AuditTerminalNext]_vars

(***************************************************************************)
(* The resume run: one identity through an explicit resume.                *)
(***************************************************************************)

RBindReplacing ==
    \E s \in {Sess1, Sess2}, r \in {Sess1, Sess2} :
        RecoverMemberSessionBindingReplacingRunning(Id1, Rid1, s, Some(r))
REndpoint == RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, E1Inproc)
RReady == ObserveRuntimeReady(Rid1, 1, Epoch1)

RResumeArms ==
    \/ BeginExplicitResumeStopped(AuditResumeAttempt)
    \/ SettleExplicitResumePreparationStopped(AuditResumeAttempt)
    \/ AuthorizeExplicitResumeMemberRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
    \/ AuthorizeExplicitResumeMemberLiveRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
    \/ AuthorizeExplicitResumeMemberJoinRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
    \/ \E obs \in {"DurableSnapshotPresent", "DurableSnapshotMissing"} :
           \/ ClassifyExplicitResumeMemberLiveRevivable(AuditResumeAttempt, Id1, AuditResumeBinding, obs, AuditReason)
           \/ ClassifyExplicitResumeMemberLiveMissing(AuditResumeAttempt, Id1, AuditResumeBinding, obs, AuditReason)
    \/ SettleExplicitResumeMemberRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
    \/ SettleExplicitResumeMemberReplacedRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
    \/ BeginExplicitResumeReadinessRunning(AuditResumeAttempt)
    \/ SettleExplicitResumeReadinessRunning(AuditResumeAttempt)
    \/ BeginExplicitResumeTopologyRunning(AuditResumeAttempt)
    \/ SettleExplicitResumeTopologyRunning(AuditResumeAttempt)
    \/ FinishExplicitResumeRunning(AuditResumeAttempt)

RRecoverArms == WRecover \/ WBind \/ RBindReplacing \/ REndpoint

AuditResumeNext ==
    \/ WRecover
    \/ WBind
    \/ RBindReplacing
    \/ REndpoint
    \/ AuditPlan(Id1)
    \/ AuditWake(Id1)
    \/ AuditResolve(Id1, Rid1, 1, 0, {Sess1, Sess2})
    \/ RReady
    \/ AuditQuiesceStop
    \/ StopRunning
    \/ ResumeStopped
    \/ RResumeArms

AuditResumeSpec == Init /\ [][AuditResumeNext]_vars

(***************************************************************************)
(* The edge run: two identities and one wiring edge (I8).                  *)
(***************************************************************************)

\* Member 1: Rid1, fence 1, session 1, endpoint 1; its next incarnation Rid3,
\* fence 3, generation 1, session 3. Member 2: Rid2, fence 2, session 2,
\* endpoint 2.
ERecover ==
    \/ RecoverRosterMemberRunning(Id1, Rid1, 1, 0, AuditProfile, AuditMode, FALSE)
    \/ RecoverRosterMemberRunning(Id1, Rid3, 3, 1, AuditProfile, AuditMode, FALSE)
    \/ RecoverRosterMemberRunning(Id2, Rid2, 2, 0, AuditProfile, AuditMode, FALSE)
EBind ==
    \/ RecoverMemberSessionBindingFreshRunning(Id1, Rid1, Sess1, None)
    \/ RecoverMemberSessionBindingFreshRunning(Id1, Rid3, Sess3, None)
    \/ RecoverMemberSessionBindingFreshRunning(Id2, Rid2, Sess2, None)
EEndpoint ==
    \/ RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, E1Inproc)
    \/ RecoverMemberPeerEndpointFreshRunning(Id1, Rid3, Sess3, E3Inproc)
    \/ RecoverMemberPeerEndpointFreshRunning(Id2, Rid2, Sess2, E2Inproc)
EWire == RecoverRosterWiringRunning(AuditEdge) \/ WireMembersRunning(AuditEdge)
EDetach == SessionIngressDetachedForMobDestroyRunning(AuditMob, Rid1)
\* The recovered members are Dormant, so their retirement is archive-only.
ERetire ==
    \/ RetireRunningReleasingArchiveOnly(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningPreservingBindingArchiveOnly(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
EArchive ==
    \/ ObserveMemberRetirementArchivedLive(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedLivePreservingTopology(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, TRUE)

ERecoverArms == ERecover \/ EBind \/ EEndpoint

AuditEdgeNext ==
    \/ ERecover
    \/ EBind
    \/ EEndpoint
    \/ EWire
    \/ SealBootMaterializationPlan
    \/ AuditPlan(Id1)
    \/ AuditPlan(Id2)
    \/ ERetire
    \/ EDetach
    \/ EArchive

AuditEdgeSpec == Init /\ [][AuditEdgeNext]_vars

(***************************************************************************)
(* The seat run: two identities from the spawn ladder's Opened phase.      *)
(***************************************************************************)

SAuthorize(id) ==
    AuthorizeSpawnProfileRunning(id, AuditProfile, AuditModel, AuditDigest, AuditDigest, AuditDigest, None, None, FALSE, None)

\* The fresh local spawn opener at generation 0, with every capability flag
\* off: the seat and the embodied commit both start at Opened.
SOpen(id, rid, fence, s) ==
    BeginSpawnExecFresh(id, rid, fence, 0, AuditDigest, FALSE, AuditMode, Some(s), None, None,
                        FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, FALSE,
                        None, None, None, None, FALSE, FALSE)

\* Member 1: Rid1, fence 1, session 1; member 2: Rid2, fence 2, session 2.
SIncarnation(id) ==
    IF id = Id1 THEN [rid |-> Rid1, fence |-> 1, session |-> Sess1]
    ELSE [rid |-> Rid2, fence |-> 2, session |-> Sess2]

SOpenArm == \E id \in {Id1, Id2} : SOpen(id, SIncarnation(id).rid, SIncarnation(id).fence, SIncarnation(id).session)

\* A seat with member 1's endpoint (member 2 then offers a duplicate peer id,
\* which R5 refuses), member 2's own endpoint, or none (EndpointPending). A
\* seat owes no kickoff (R13).
SSeat ==
    \E id \in {Id1, Id2}, ep \in {Some(E1Inproc), Some(E2Inproc), None} :
        CommitDormantSeatRunning(id, SIncarnation(id).rid, SIncarnation(id).fence, 0, FALSE, AuditMode,
                                 SIncarnation(id).session, ep, FALSE, FALSE)

SCommit ==
    \E id \in {Id1, Id2} :
        CommitSpawnMembershipFresh(id, SIncarnation(id).rid, SIncarnation(id).fence, 0, AuditDigest, FALSE, AuditMode,
                                   Some(SIncarnation(id).session), None, None, None, None, None, None)

SReady == \E id \in {Id1, Id2} : ObserveRuntimeReady(SIncarnation(id).rid, SIncarnation(id).fence, Epoch1)

SRelease ==
    \E id \in {Id1, Id2} :
        ReleaseMemberMaterializationReleasedRunning(Some(SIncarnation(id).rid), Some(SIncarnation(id).fence),
                                                    Epoch1, "RegistrationReleased")

SRegisterPeer ==
    \/ RegisterMemberPeerRunning(Id1, Rid1, 0, 1, E1Inproc)
    \/ RegisterMemberPeerRunning(Id2, Rid2, 0, 2, E2Inproc)

\* Member 1's rotation onto session 3 (endpoint 3 or none), its next
\* incarnation Rid3, fence 3, generation 1. A seated member is Dormant until
\* it wakes, so its retirement may be archive-only.
SRetire ==
    \/ RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningReleasingArchiveOnly(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
SBeginRotation ==
    \E ep \in {Some(E3Inproc), None} : BeginMemberRotationRunning(Id1, Rid1, 1, 0, Sess3, ep, FALSE)
SRuntimeRetired == ObserveRuntimeRetired(Rid1, 1)
SDetach == SessionIngressDetachedForMobDestroyRunning(AuditMob, Rid1)
SCommitRotation ==
    \/ CommitMemberRotationLiveRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
    \/ CommitMemberRotationRetiredRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)

SRecoverArms == SSeat \/ SCommitRotation

AuditSeatNext ==
    \/ \E id \in {Id1, Id2} : SAuthorize(id)
    \/ SOpenArm
    \/ SSeat
    \/ SCommit
    \/ SReady
    \/ SRelease
    \/ SRegisterPeer
    \/ AuditPlan(Id1)
    \/ AuditPlan(Id2)
    \/ SealBootMaterializationPlan
    \/ SRetire
    \/ SBeginRotation
    \/ SRuntimeRetired
    \/ SDetach
    \/ SCommitRotation

AuditSeatSpec == Init /\ [][AuditSeatNext]_vars

\* A dormant seat leaves its identity Dormant, with its endpoint or owed
\* EndpointPending, and requests no runtime binding.
AuditSeatLeavesDormant ==
    [][\A id \in {Id1, Id2}, ep \in {Some(E1Inproc), Some(E2Inproc), None} :
         CommitDormantSeatRunning(id, SIncarnation(id).rid, SIncarnation(id).fence, 0, FALSE, AuditMode,
                                  SIncarnation(id).session, ep, FALSE, FALSE)
           => /\ id \in DOMAIN member_materialization'
              /\ member_materialization'[id] = "Dormant"
              /\ \/ id \in DOMAIN member_peer_endpoints'
                 \/ /\ id \in DOMAIN member_boot_obligation_owed'
                    /\ member_boot_obligation_owed'[id] = "EndpointPending"
              /\ SIncarnation(id).rid \notin member_startup_binding_requested'
      ]_vars

(***************************************************************************)
(* The wake2 run: two identities woken beside each other.                  *)
(***************************************************************************)

W2Recover ==
    \/ RecoverRosterMemberRunning(Id1, Rid1, 1, 0, AuditProfile, AuditMode, FALSE)
    \/ RecoverRosterMemberRunning(Id2, Rid2, 2, 0, AuditProfile, AuditMode, FALSE)
W2Bind ==
    \/ RecoverMemberSessionBindingFreshRunning(Id1, Rid1, Sess1, None)
    \/ RecoverMemberSessionBindingFreshRunning(Id2, Rid2, Sess2, None)
W2Endpoint ==
    \/ RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, E1Inproc)
    \/ RecoverMemberPeerEndpointFreshRunning(Id2, Rid2, Sess2, E2Inproc)
\* A peer delivery from a sender that is or is not Waking, and a host delivery.
W2Wake ==
    \E id \in {Id1, Id2}, sw \in BOOLEAN :
        \/ WakeArm(id, "PeerDelivery", FALSE, "Content", sw)
        \/ WakeArm(id, "HostDelivery", FALSE, "Content", FALSE)
W2Ready ==
    \/ ObserveRuntimeReady(Rid1, 1, Epoch1)
    \/ ObserveRuntimeReady(Rid2, 2, Epoch1)
W2Release ==
    \/ ReleaseMemberMaterializationReleasedRunning(Some(Rid1), Some(1), Epoch1, "RegistrationReleased")
    \/ ReleaseMemberMaterializationReleasedRunning(Some(Rid2), Some(2), Epoch1, "RegistrationReleased")
\* A notice from one member to the other.
W2Notice ==
    \E ic \in AuditInputClasses :
        \/ ClassifyPeerNoticeRunning(Id1, Id2, ic)
        \/ ClassifyPeerNoticeRunning(Id2, Id1, ic)
\* Member 1's retirement while member 2 is Waking.
W2Retire ==
    \/ RetireRunningPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireRunningPreservingBindingArchiveOnly(AuditMob, Rid1, Id1, 0, None, Some(Sess1))

AuditWake2Next ==
    \/ W2Recover
    \/ W2Bind
    \/ W2Endpoint
    \/ W2Wake
    \/ AuditWakePath(Id1)
    \/ AuditWakePath(Id2)
    \/ W2Ready
    \/ W2Release
    \/ W2Notice
    \/ W2Retire

AuditWake2Spec == Init /\ [][AuditWake2Next]_vars

(***************************************************************************)
(* Constraint and view.                                                    *)
(***************************************************************************)

AuditStateConstraint == member_wake_attempt_sequence <= AuditMaxAttempts

\* Every state variable but the two counters (see the header).
AuditView == [vars EXCEPT ![AuditStepIndex] = 0, ![AuditEpochIndex] = 0]

(***************************************************************************)
(* Properties.                                                             *)
(***************************************************************************)

\* AP1: a recovery or seat step leaves every existing phase as it was (a
\* phase may only be added, by the one seating arm, or removed, by an unseat).
PhaseKept ==
    \A id \in DOMAIN member_materialization \cap DOMAIN member_materialization' :
        member_materialization'[id] = member_materialization[id]

AuditSeedRecoverKeepsPhase == [][SeedRecoverArms => PhaseKept]_vars
AuditRecoverKeepsPhase == [][WRecoverArms => PhaseKept]_vars
AuditTerminalRecoverKeepsPhase == [][TRecoverArms => PhaseKept]_vars
AuditResumeRecoverKeepsPhase == [][RRecoverArms => PhaseKept]_vars
AuditEdgeRecoverKeepsPhase == [][ERecoverArms => PhaseKept]_vars
AuditSeatRecoverKeepsPhase == [][SRecoverArms => PhaseKept]_vars

\* AP2 (safety form): an open attempt is never replaced, and it disappears
\* only in a step that leaves its member Live, Dormant or without a phase.
AuditAttemptEndsClosed ==
    [][\A id \in DOMAIN member_wake_attempts :
         \/ /\ id \in DOMAIN member_wake_attempts'
            /\ member_wake_attempts'[id] = member_wake_attempts[id]
         \/ /\ id \notin DOMAIN member_wake_attempts'
            /\ \/ id \notin DOMAIN member_materialization'
               \/ member_materialization'[id] \in {"Live", "Dormant"}
      ]_vars

\* AP2 (ENABLED form): every Waking member has an enabled closing arm
\* for every closing kind, whatever the preconditions of its other feedback,
\* so the shell can always close an attempt whose Bound or Succeeded step has
\* no arm.
AuditWakingClosable ==
    \A id \in DOMAIN member_wake_attempts :
        LET a == member_wake_attempts[id] IN
        /\ ENABLED AbandonMemberWakeDormantRunning(id, a, "Unsettled", AuditReason)
        /\ ENABLED AbandonMemberWakeBrokenRunning(id, a, "ReadinessFailed", AuditReason)
        /\ ENABLED ResolveMemberRevivalFailedRunning(id, AuditReason, a)
        /\ ENABLED ResolveMemberRevivalHostRefusedRunning(id, AuditCode, AuditReason, a)
        /\ ENABLED ResolveMemberRevivalRetryableRunning(id, a, "WakeTimedOut")

\* AP3: a Lifecycle wake input never opens an attempt, and a peer notice
\* classification never changes residency.
AuditLifecycleNeverWakes ==
    [][/\ (\E s \in AuditWakeShapes : s[3] = "Lifecycle" /\ WakeArm(Id1, s[1], s[2], s[3], s[4]))
           => member_wake_attempts' = member_wake_attempts
       /\ AuditNotice(Id1)
           => /\ member_wake_attempts' = member_wake_attempts
              /\ member_materialization' = member_materialization
      ]_vars

\* A WakeMember input never moves a Dormant member straight to Live: it is
\* Waking (an attempt) or stays Dormant (refused).
AuditWakeNeverSkipsWaking ==
    [][AuditWakeShaped(Id1)
           => (Phase1 = Some("Dormant") => Phase1Next \in {Some("Dormant"), Some("Waking")})
      ]_vars

AuditWakeVerdicts(id, s) ==
    <<ENABLED WakeMemberAuthorizedRunning(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberJoinedRunning(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberAlreadyLiveRunning(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberRefusedRunning(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberRefusedStopped(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberRefusedCompleted(id, s[1], s[2], s[3], s[4]),
      ENABLED WakeMemberRefusedDestroyed(id, s[1], s[2], s[3], s[4])>>

\* WakeMember is total and its arms never overlap, for every input shape.
AuditWakeExactlyOne ==
    \A s \in AuditWakeShapes :
        Cardinality({i \in 1..7 : AuditWakeVerdicts(Id1, s)[i]}) = 1

\* Member 1 owes EndpointPending (before a step).
Owed1EndpointPending ==
    /\ Id1 \in DOMAIN member_boot_obligation_owed
    /\ member_boot_obligation_owed[Id1] = "EndpointPending"

\* A local wake never closes Live without an endpoint while its member owes
\* EndpointPending, so a Live member is reachable.
AuditSucceededLeavesEndpoint ==
    [][(\E a \in AuditAttempts : ResolveMemberRevivalSucceededRunningLocal(Id1, a))
           => (Id1 \in DOMAIN member_peer_endpoints' \/ ~Owed1EndpointPending)
      ]_vars

\* A bound local wake of a member owing no endpoint cause can always close
\* Live: it is never held Waking for an endpoint.
AuditWakeClosesLiveUnlessEndpointOwed ==
    \A id \in DOMAIN member_wake_attempts :
        (/\ phase = "Running"
         /\ id \in member_wake_bound
         /\ id \notin DOMAIN member_placement
         /\ ~(/\ id \in DOMAIN member_boot_obligation_owed
              /\ member_boot_obligation_owed[id] = "EndpointPending"))
            => ENABLED ResolveMemberRevivalSucceededRunningLocal(id, member_wake_attempts[id])

\* The boot plan never leaves a member that blocks the seal: after its plan a
\* member has a durable endpoint or owes EndpointPending.
AuditPlanLeavesSealable ==
    [][AuditPlan(Id1)
           => \/ Id1 \in DOMAIN member_peer_endpoints'
              \/ /\ Id1 \in DOMAIN member_boot_obligation_owed'
                 /\ member_boot_obligation_owed'[Id1] = "EndpointPending"
      ]_vars

\* A release whose epoch is not member 1's recorded epoch leaves its phase
\* as it was (the epoch fence; a stale release after a newer wake never
\* demotes it).
AuditReleaseEpochFence ==
    [][\A ep \in AuditReleaseEpochs :
         (/\ ReleaseArm(Some(Rid1), Some(1), ep)
          /\ ~(Id1 \in DOMAIN member_materialization_epoch /\ member_materialization_epoch[Id1] = ep))
           => Phase1Next = Phase1
      ]_vars

\* R27: no arm routes a runtime retire for a member with no registration (a
\* Dormant or Waking member, or an archive-only retirement).
AuditRetireRoutesOnlyRegistered ==
    [][RoutingRetire
           => /\ Phase1 \notin {Some("Dormant"), Some("Waking")}
              /\ Rid1 \notin runtime_retire_archive_only
      ]_vars

\* Member 1's retirement waits only for its archive: the incarnation is
\* Retiring with its pending session and its ingress detach closed.
RetiringRid1Settled ==
    /\ phase \in {"Running", "Stopped"}
    /\ Id1 \in DOMAIN identity_to_runtime
    /\ identity_to_runtime[Id1] = Rid1
    /\ Rid1 \in DOMAIN member_state_markers
    /\ member_state_markers[Rid1] = "Retiring"
    /\ Rid1 \in DOMAIN runtime_retire_pending_sessions
    /\ runtime_retire_pending_sessions[Rid1] = Sess1
    /\ Rid1 \notin pending_session_ingress_detach_runtime_ids

ArchiveEnabled ==
    \/ ENABLED ObserveMemberRetirementArchivedLive(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ENABLED ObserveMemberRetirementArchivedRetired(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ENABLED ObserveMemberRetirementArchivedLiveStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ENABLED ObserveMemberRetirementArchivedRetiredStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)

\* R124 (and R27): a settled retirement can always archive. Outside a destroy
\* a rotation or a pending host carry-out may hold the plain terminal (the
\* rotation's own terminal re-seats the identity); under an admitted destroy
\* nothing holds it: the destroy beats the rotation and the host carry-out.
AuditRetirementArchivable ==
    (/\ RetiringRid1Settled
     /\ \/ destroy_admitted
        \/ /\ Id1 \notin DOMAIN member_rotation_session_ids
           /\ Id1 \notin member_host_preparation_pending)
        => ArchiveEnabled

\* R124: no rotation begins or commits under an admitted destroy.
AuditDestroyFencesRotation == [][destroy_admitted => ~RotationArms]_vars

\* I5: over the wake refusal helper's whole input domain, a present,
\* session-bound member with a phase is never refused as not a member.
ASSUME \A present, bound, placed, broken, retiring, admitting, authoritative, origin_open,
          host_pending, sender_waking, observed_missing \in BOOLEAN,
          input_class \in {"Content", "Lifecycle"},
          residency \in {Some("Dormant"), Some("Waking"), Some("Live")} :
    (present /\ bound) =>
        member_wake_refusal(present, bound, placed, broken, retiring, admitting, authoritative, origin_open,
                            host_pending, input_class, sender_waking, observed_missing, residency)
            # Some("NotAMember")

\* The release verdict: Release exactly for a seated, local, Live member, and
\* WakeInFlight exactly for a seated, local, Waking one.
ASSUME \A seated, placed \in BOOLEAN,
          residency \in {None, Some("Dormant"), Some("Waking"), Some("Live")} :
    /\ (member_release_verdict(seated, placed, residency) = "Release")
           <=> (seated /\ ~placed /\ residency = Some("Live"))
    /\ (member_release_verdict(seated, placed, residency) = "WakeInFlight")
           <=> (seated /\ ~placed /\ residency = Some("Waking"))

(***************************************************************************)
(* Reachability goals, each checked as an expected violation of its       *)
(* negation: a state goal as an invariant, a step goal as an action        *)
(* property.                                                               *)
(***************************************************************************)

\* Seed run: a sealed plan leaves the member Dormant.
AuditWitnessSealedDormant == boot_plan_sealed /\ Phase1 = Some("Dormant")
\* Seed run: a sealed member with no endpoint is owed EndpointPending.
AuditWitnessSealedEndpointPending ==
    /\ boot_plan_sealed
    /\ Id1 \in DOMAIN member_boot_obligation_owed
    /\ member_boot_obligation_owed[Id1] = "EndpointPending"
\* Wake run: a wake attempt closes Live.
AuditWitnessWokeLiveStep == Phase1 = Some("Waking") /\ Phase1Next = Some("Live")
\* Wake run: a registration release returns a Live member to Dormant.
AuditWitnessReleasedStep ==
    /\ Phase1 = Some("Live")
    /\ Phase1Next = Some("Dormant")
    /\ \E ep \in AuditReleaseEpochs : ReleaseArm(Some(Rid1), Some(1), ep)
\* Wake run: a Stop closes an open attempt.
AuditWitnessStopClosesStep == StopRunning /\ member_wake_attempts # [x \in {} |-> None]
\* Wake run: late feedback for an attempt a Stop closed is a stale no-op in Stopped.
AuditWitnessStaleInStopped ==
    /\ phase = "Stopped"
    /\ member_wake_attempt_sequence > 0
    /\ ENABLED ResolveMemberRevivalBoundStaleStopped(Id1, member_wake_attempt_sequence)
\* Wake run: the drift arm reopens a Live member's wake.
AuditWitnessDrift == member_materialization_drift_count > 0
\* Wake run: a wake verdict records Broken.
AuditWitnessBrokenWake == Id1 \in DOMAIN member_restore_failures /\ Id1 \in DOMAIN identity_to_runtime
\* Wake run: a member owing no endpoint cause closes Live before an endpoint is
\* registered (the shape of the owed-endpoint guard).
AuditWitnessCommsLessWokeLiveStep ==
    /\ Phase1 = Some("Waking")
    /\ Phase1Next = Some("Live")
    /\ Id1 \notin DOMAIN member_peer_endpoints'

\* Retire run: a retirement start closes an open attempt (archive-only).
AuditWitnessRetireClosesStep ==
    /\ Phase1 = Some("Waking")
    /\ RetireArms
\* Retire run: an archive-only retirement is archived.
AuditWitnessArchiveOnlyArchivedStep ==
    /\ Rid1 \in runtime_retire_archive_only
    /\ PlainArchives
    /\ Id1 \notin DOMAIN identity_to_runtime'
\* Retire run: under an admitted destroy the plain terminal archives a
\* rotating identity and drops its intent.
AuditWitnessDestroyBeatsRotationStep ==
    /\ Id1 \in DOMAIN member_rotation_session_ids
    /\ destroy_admitted
    /\ PlainArchives
\* Retire run: under an admitted destroy the plain terminal archives a
\* host-pending identity.
AuditWitnessDestroyBeatsHostPendingStep ==
    /\ Id1 \in member_host_preparation_pending
    /\ destroy_admitted
    /\ PlainArchives
\* Retire run: a Stop that landed mid-rotation lets the terminal finish it.
AuditWitnessRotatedStoppedStep ==
    \/ CommitMemberRotationLiveStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)
    \/ CommitMemberRotationRetiredStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE, FALSE, AuditObjective)

\* Resume run: an explicit resume joins a wake already in flight.
AuditWitnessResumeJoinsStep == AuthorizeExplicitResumeMemberJoinRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
\* Resume run: an explicit resume reserves a Live member.
AuditWitnessResumeReservesLiveStep == AuthorizeExplicitResumeMemberLiveRunning(AuditResumeAttempt, Id1, AuditResumeBinding)
\* Resume run: the resume finishes with its member Live.
AuditWitnessResumeFinishedLiveStep ==
    /\ FinishExplicitResumeRunning(AuditResumeAttempt)
    /\ Phase1 = Some("Live")

\* Edge run: after the seal an edge endpoint is unseated and held by the
\* preserved respawn topology.
AuditWitnessHeldEdge ==
    /\ boot_plan_sealed
    /\ AuditEdge \in wiring_edges
    /\ Id1 \notin DOMAIN identity_to_runtime
    /\ Id1 \in pending_respawn_topology

\* Seat run: both identities are seated Dormant.
AuditWitnessBothSeated ==
    /\ Id1 \in DOMAIN member_materialization /\ member_materialization[Id1] = "Dormant"
    /\ Id2 \in DOMAIN member_materialization /\ member_materialization[Id2] = "Dormant"
\* Seat run: a seat with no endpoint is owed EndpointPending.
AuditWitnessSeatEndpointPending ==
    \E id \in {Id1, Id2} :
        /\ id \in DOMAIN member_boot_obligation_owed
        /\ member_boot_obligation_owed[id] = "EndpointPending"
        /\ id \notin DOMAIN member_peer_endpoints
\* Seat run: a rotation re-seats the next incarnation Dormant.
AuditWitnessRotatedStep == SCommitRotation
\* Seat run: a release returns a spawned (Live) member to Dormant and clears
\* its startup ladder (R12).
AuditWitnessReleasedSpawnStep ==
    /\ SRelease
    /\ \E rid \in {Rid1, Rid2} :
           /\ rid \in member_startup_runtime_ready
           /\ rid \notin member_startup_runtime_ready'

\* Wake2 run: a notice between members while the sender is Waking.
AuditWitnessCrossNoticeStep ==
    /\ Phase1 = Some("Waking")
    /\ \E ic \in AuditInputClasses : ClassifyPeerNoticeRunning(Id2, Id1, ic)
\* Wake2 run: member 1 retires while member 2 is Waking.
AuditWitnessRetireBesideWakingStep ==
    /\ Phase2 = Some("Waking")
    /\ W2Retire

NotAuditWitnessSealedDormant == ~AuditWitnessSealedDormant
NotAuditWitnessSealedEndpointPending == ~AuditWitnessSealedEndpointPending
NotAuditWitnessWokeLive == [][~AuditWitnessWokeLiveStep]_vars
NotAuditWitnessReleased == [][~AuditWitnessReleasedStep]_vars
NotAuditWitnessStopCloses == [][~AuditWitnessStopClosesStep]_vars
NotAuditWitnessStaleInStopped == ~AuditWitnessStaleInStopped
NotAuditWitnessDrift == ~AuditWitnessDrift
NotAuditWitnessBrokenWake == ~AuditWitnessBrokenWake
NotAuditWitnessCommsLessWokeLive == [][~AuditWitnessCommsLessWokeLiveStep]_vars
NotAuditWitnessRetireCloses == [][~AuditWitnessRetireClosesStep]_vars
NotAuditWitnessArchiveOnlyArchived == [][~AuditWitnessArchiveOnlyArchivedStep]_vars
NotAuditWitnessDestroyBeatsRotation == [][~AuditWitnessDestroyBeatsRotationStep]_vars
NotAuditWitnessDestroyBeatsHostPending == [][~AuditWitnessDestroyBeatsHostPendingStep]_vars
NotAuditWitnessRotatedStopped == [][~AuditWitnessRotatedStoppedStep]_vars
NotAuditWitnessResumeJoins == [][~AuditWitnessResumeJoinsStep]_vars
NotAuditWitnessResumeReservesLive == [][~AuditWitnessResumeReservesLiveStep]_vars
NotAuditWitnessResumeFinishedLive == [][~AuditWitnessResumeFinishedLiveStep]_vars
NotAuditWitnessHeldEdge == ~AuditWitnessHeldEdge
NotAuditWitnessBothSeated == ~AuditWitnessBothSeated
NotAuditWitnessSeatEndpointPending == ~AuditWitnessSeatEndpointPending
NotAuditWitnessRotated == [][~AuditWitnessRotatedStep]_vars
NotAuditWitnessReleasedSpawn == [][~AuditWitnessReleasedSpawnStep]_vars
NotAuditWitnessCrossNotice == [][~AuditWitnessCrossNoticeStep]_vars
NotAuditWitnessRetireBesideWaking == [][~AuditWitnessRetireBesideWakingStep]_vars
====
