---- MODULE member_residency_audit ----
\* Hand-written audit of MobMachine member residency (Dormant, Waking, Live).
\*
\* The generated ci profile has no identities, so the residency arms and the
\* residency invariants (I1-I14) hold there only vacuously. This audit keeps
\* the generated model unchanged and drives the residency arms in four runs:
\*
\*   wake (AuditWakeSpec): one identity, one incarnation. The member is
\*     recovered, bound (the one seating arm: Dormant), given an inproc or a
\*     network endpoint or none, and seeded with every restore policy,
\*     composition authority and host-pending set. It takes every boot verdict
\*     (Wake, VerifyThenDormant, StayDormant) and the seal, every WakeMember arm
\*     in every lifecycle phase, every attempt-fenced resolution (Bound,
\*     Succeeded, Failed, HostRefused, Retryable, both Abandon arms, the
\*     recreated-session Bound step) and its StaleAttempt twin in every phase,
\*     the runtime-ready observation with and without an epoch, every release
\*     shape in every phase, the drift and terminal classifications, the peer
\*     notice classification, Stop and Resume, Complete, the destroy admission
\*     and DestroyMob, Shutdown, a retirement start (releasing and preserving
\*     the binding) with its archive, and the replayed Retired record.
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
\*
\* Domains are fixed per run (see the runner). Each run explores its whole
\* reachable space under a state constraint that caps the machine's wake
\* attempt mint (member_wake_attempt_sequence <= AuditMaxAttempts) and a VIEW
\* that leaves out model_step_count and topology_epoch, which no guard reads.
\*
\* Properties, in every run:
\*   every generated invariant of ci.cfg (kept), I1-I14 among them;
\*   AuditRecoverKeepsPhase (AP1, action property): no recovery or seat arm
\*     overwrites an existing residency phase;
\*   AuditAttemptEndsClosed (AP2, action property, safety form): an open wake
\*     attempt is never replaced, and it disappears only in a step that leaves
\*     its member Live, Dormant or without a phase (Broken, retiring, unseated);
\*   AuditWakingClosable (invariant, the ENABLED form of AP2): every Waking
\*     member has an enabled closing arm (the shell's Unsettled abandon).
\* In the wake run also:
\*   AuditLifecycleNeverWakes (AP3, action property): a Lifecycle wake input
\*     never opens an attempt, and a peer notice classification never changes
\*     residency;
\*   AuditWakeExactlyOne (invariant): for every wake input exactly one
\*     WakeMember arm is enabled (the arms are total and disjoint).
\* I5 is an ASSUME over the generated wake refusal helper's whole input domain:
\* a present, session-bound member with a phase is never refused as not a
\* member.
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
AuditEpochs == {None, Some("runtimeepochid_1")}
AuditAttempts == 1..AuditMaxAttempts
AuditInputClasses == {"Content", "Lifecycle"}

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
WakeArm(id, om, ic, sw) ==
    \/ WakeMemberAuthorizedRunning(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberJoinedRunning(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberAlreadyLiveRunning(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberRefusedRunning(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberRefusedStopped(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberRefusedCompleted(id, "PeerDelivery", om, ic, sw)
    \/ WakeMemberRefusedDestroyed(id, "PeerDelivery", om, ic, sw)

AuditWake(id) == \E om \in BOOLEAN, ic \in AuditInputClasses, sw \in BOOLEAN : WakeArm(id, om, ic, sw)

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

\* Late or stale feedback in every lifecycle phase: a typed StaleAttempt no-op.
AuditStaleFeedback(id, rid, fence, g, s) ==
    \E a \in AuditAttempts :
        \/ ResolveMemberRevivalBoundStaleRunning(id, a)
        \/ ResolveMemberRevivalBoundStaleStopped(id, a)
        \/ ResolveMemberRevivalBoundStaleCompleted(id, a)
        \/ ResolveMemberRevivalBoundStaleDestroyed(id, a)
        \/ ResolveMemberRevivalSucceededStaleRunning(id, a)
        \/ ResolveMemberRevivalSucceededStaleStopped(id, a)
        \/ ResolveMemberRevivalSucceededStaleCompleted(id, a)
        \/ ResolveMemberRevivalSucceededStaleDestroyed(id, a)
        \/ ResolveMemberRevivalFailedStaleRunning(id, AuditReason, a)
        \/ ResolveMemberRevivalFailedStaleStopped(id, AuditReason, a)
        \/ ResolveMemberRevivalFailedStaleCompleted(id, AuditReason, a)
        \/ ResolveMemberRevivalFailedStaleDestroyed(id, AuditReason, a)
        \/ ResolveMemberRevivalHostRefusedStaleRunning(id, AuditCode, AuditReason, a)
        \/ ResolveMemberRevivalHostRefusedStaleStopped(id, AuditCode, AuditReason, a)
        \/ ResolveMemberRevivalHostRefusedStaleCompleted(id, AuditCode, AuditReason, a)
        \/ ResolveMemberRevivalHostRefusedStaleDestroyed(id, AuditCode, AuditReason, a)
        \/ ResolveMemberRevivalRetryableStaleRunning(id, a, "WakeTimedOut")
        \/ ResolveMemberRevivalRetryableStaleStopped(id, a, "WakeTimedOut")
        \/ ResolveMemberRevivalRetryableStaleCompleted(id, a, "WakeTimedOut")
        \/ ResolveMemberRevivalRetryableStaleDestroyed(id, a, "WakeTimedOut")
        \/ AbandonMemberWakeStaleRunning(id, a, "Unsettled", AuditReason)
        \/ AbandonMemberWakeStaleStopped(id, a, "Unsettled", AuditReason)
        \/ AbandonMemberWakeStaleCompleted(id, a, "Unsettled", AuditReason)
        \/ AbandonMemberWakeStaleDestroyed(id, a, "Unsettled", AuditReason)
        \/ ResolveRecreatedMemberSessionSucceededStaleRunning(id, rid, fence, g, s, a)
        \/ ResolveRecreatedMemberSessionSucceededStaleStopped(id, rid, fence, g, s, a)
        \/ ResolveRecreatedMemberSessionSucceededStaleCompleted(id, rid, fence, g, s, a)
        \/ ResolveRecreatedMemberSessionSucceededStaleDestroyed(id, rid, fence, g, s, a)

\* Every release arm of one shape.
ReleaseArm(rid, fence, ep) ==
    \/ ReleaseMemberMaterializationReleasedRunning(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationReleasedStopped(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationReleasedCompleted(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleRunning(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleStopped(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleCompleted(rid, fence, ep, "RegistrationReleased")
    \/ ReleaseMemberMaterializationStaleDestroyed(rid, fence, ep, "RegistrationReleased")

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

AuditHostPrepared(id) ==
    \/ ResolveMemberHostPreparationRunning(id)
    \/ ResolveMemberHostPreparationStopped(id)
    \/ ResolveMemberHostPreparationCompleted(id)

AuditNotice(id) ==
    \E ic \in AuditInputClasses :
        \/ ClassifyPeerNoticeRunning(id, id, ic)
        \/ ClassifyPeerNoticeStopped(id, id, ic)
        \/ ClassifyPeerNoticeCompleted(id, id, ic)
        \/ ClassifyPeerNoticeDestroyed(id, id, ic)

AuditClassify(id) ==
    \E obs \in {"DurableSnapshotPresent", "DurableSnapshotMissing"} :
        \/ ClassifyMemberLiveMaterializationRevivable(id, obs, AuditReason)
        \/ ClassifyMemberLiveMaterializationTerminal(id, obs, AuditReason)
        \/ ClassifyMemberLiveMaterializationTerminalDormant(id, obs, AuditReason)

AuditQuiesceStop == BeginPlacedCompletionLifecycleQuiesceFreshStop("Stop")
AuditQuiesceComplete == BeginPlacedCompletionLifecycleQuiesceFresh("Complete")
AuditQuiesceDestroy == BeginPlacedCompletionLifecycleQuiesceFresh("Destroy")

(***************************************************************************)
(* The wake run: one identity, one incarnation (Rid1, fence 1, generation  *)
(* 0, session 1).                                                          *)
(***************************************************************************)

WRecover == RecoverRosterMemberRunning(Id1, Rid1, 1, 0, AuditProfile, AuditMode, FALSE)
WBind == RecoverMemberSessionBindingFreshRunning(Id1, Rid1, Sess1, None)
WBindCurrent == RecoverMemberSessionBindingAlreadyCurrentRunning(Id1, Rid1, Sess1, None)
WEndpoint == \E e \in {E1Inproc, E1Tcp} : RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, e)
WRegisterPeer == \E e \in {E1Inproc, E1Tcp} : RegisterMemberPeerRunning(Id1, Rid1, 0, 1, e)
WRecoverRetired == \E p \in BOOLEAN, started \in BOOLEAN : RecoverRosterMemberRetiredRunning(Id1, Rid1, 0, p, started)
WReady == \E ep \in AuditEpochs : ObserveRuntimeReady(Rid1, 1, ep)
WRelease == \E rid \in {Some(Rid1), None}, fence \in {Some(1), None}, ep \in AuditEpochs : ReleaseArm(rid, fence, ep)
WRetire ==
    \/ RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
    \/ RetireStoppedReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
WRuntimeRetired == ObserveRuntimeRetired(Rid1, 1) \/ ObserveRuntimeRetiredStopped(Rid1, 1)
WDetach ==
    \/ SessionIngressDetachedForMobDestroyRunning(AuditMob, Rid1)
    \/ SessionIngressDetachedForMobDestroyStopped(AuditMob, Rid1)
WArchive ==
    \/ ObserveMemberRetirementArchivedLive(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedRetired(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedLiveStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)
    \/ ObserveMemberRetirementArchivedRetiredStopped(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, FALSE)

\* The recovery and seat arms of the wake run (AP1).
WRecoverArms == WRecover \/ WBind \/ WBindCurrent \/ WEndpoint \/ WRecoverRetired

AuditWakeNext ==
    \/ WRecover
    \/ WBind
    \/ WBindCurrent
    \/ WEndpoint
    \/ WRegisterPeer
    \/ WRecoverRetired
    \/ AuditSeed({{}, {Id1}})
    \/ AuditHostPrepared(Id1)
    \/ AuditPlan(Id1)
    \/ SealBootMaterializationPlan
    \/ AuditWake(Id1)
    \/ AuditResolve(Id1, Rid1, 1, 0, {Sess1})
    \/ AuditStaleFeedback(Id1, Rid1, 1, 0, Sess1)
    \/ WReady
    \/ WRelease
    \/ AuditClassify(Id1)
    \/ AuditNotice(Id1)
    \/ AuditQuiesceStop
    \/ StopRunning
    \/ ResumeStopped
    \/ AuditQuiesceComplete
    \/ CompleteRunning
    \/ MarkCompleted
    \/ AuditQuiesceDestroy
    \/ AdmitDestroyCleanup
    \/ DestroyMob(Sess1)
    \/ ShutdownRunning
    \/ WRetire
    \/ WDetach
    \/ WRuntimeRetired
    \/ WArchive

AuditWakeSpec == Init /\ [][AuditWakeNext]_vars

(***************************************************************************)
(* The resume run: one identity through an explicit resume.                *)
(***************************************************************************)

RBindReplacing ==
    \E s \in {Sess1, Sess2}, r \in {Sess1, Sess2} :
        RecoverMemberSessionBindingReplacingRunning(Id1, Rid1, s, Some(r))
REndpoint == RecoverMemberPeerEndpointFreshRunning(Id1, Rid1, Sess1, E1Inproc)
RReady == ObserveRuntimeReady(Rid1, 1, Some("runtimeepochid_1"))

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
ERetire ==
    \/ RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
    \/ RetireRunningPreservingBinding(AuditMob, Rid1, Id1, 0, None, Some(Sess1))
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
\* which R5 refuses), member 2's own endpoint, or none (EndpointPending).
SSeat ==
    \E id \in {Id1, Id2}, ep \in {Some(E1Inproc), Some(E2Inproc), None} :
        CommitDormantSeatRunning(id, SIncarnation(id).rid, SIncarnation(id).fence, 0, FALSE, AuditMode,
                                 SIncarnation(id).session, ep, FALSE)

SCommit ==
    \E id \in {Id1, Id2} :
        CommitSpawnMembershipFresh(id, SIncarnation(id).rid, SIncarnation(id).fence, 0, AuditDigest, FALSE, AuditMode,
                                   Some(SIncarnation(id).session), None, None, None, None, None, None)

SReady == \E id \in {Id1, Id2} : ObserveRuntimeReady(SIncarnation(id).rid, SIncarnation(id).fence, Some("runtimeepochid_1"))

SRelease ==
    \E id \in {Id1, Id2} :
        ReleaseMemberMaterializationReleasedRunning(Some(SIncarnation(id).rid), Some(SIncarnation(id).fence),
                                                    Some("runtimeepochid_1"), "RegistrationReleased")

SRegisterPeer ==
    \/ RegisterMemberPeerRunning(Id1, Rid1, 0, 1, E1Inproc)
    \/ RegisterMemberPeerRunning(Id2, Rid2, 0, 2, E2Inproc)

\* Member 1's rotation onto session 3 (endpoint 3 or none), its next
\* incarnation Rid3, fence 3, generation 1.
SRetire == RetireRunningReleasing(AuditMob, Rid1, Id1, 0, Some(Sess1), Some(Sess1))
SBeginRotation ==
    \E ep \in {Some(E3Inproc), None} : BeginMemberRotationRunning(Id1, Rid1, 1, 0, Sess3, ep, FALSE)
SRuntimeRetired == ObserveRuntimeRetired(Rid1, 1)
SDetach == SessionIngressDetachedForMobDestroyRunning(AuditMob, Rid1)
SCommitRotation ==
    \/ CommitMemberRotationLiveRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE)
    \/ CommitMemberRotationRetiredRunning(Id1, Rid1, 1, 0, Some(Sess1), AuditDisposal, Rid3, 3, 1, AuditMode, FALSE)

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

AuditRecoverKeepsPhase == [][WRecoverArms => PhaseKept]_vars
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

\* AP2 (ENABLED form): every Waking member has an enabled closing arm.
AuditWakingClosable ==
    \A id \in DOMAIN member_wake_attempts :
        ENABLED AbandonMemberWakeDormantRunning(id, member_wake_attempts[id], "Unsettled", AuditReason)

\* AP3: a Lifecycle wake input never opens an attempt, and a peer notice
\* classification never changes residency.
AuditLifecycleNeverWakes ==
    [][/\ (\E om \in BOOLEAN, sw \in BOOLEAN : WakeArm(Id1, om, "Lifecycle", sw))
           => member_wake_attempts' = member_wake_attempts
       /\ AuditNotice(Id1)
           => /\ member_wake_attempts' = member_wake_attempts
              /\ member_materialization' = member_materialization
      ]_vars

AuditWakeVerdicts(id, om, ic, sw) ==
    <<ENABLED WakeMemberAuthorizedRunning(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberJoinedRunning(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberAlreadyLiveRunning(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberRefusedRunning(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberRefusedStopped(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberRefusedCompleted(id, "PeerDelivery", om, ic, sw),
      ENABLED WakeMemberRefusedDestroyed(id, "PeerDelivery", om, ic, sw)>>

\* WakeMember is total and its arms never overlap.
AuditWakeExactlyOne ==
    \A om \in BOOLEAN, ic \in AuditInputClasses, sw \in BOOLEAN :
        Cardinality({i \in 1..7 : AuditWakeVerdicts(Id1, om, ic, sw)[i]}) = 1

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

(***************************************************************************)
(* Reachability goals, each checked as an expected violation of its       *)
(* negation: a state goal as an invariant, a step goal as an action        *)
(* property.                                                               *)
(***************************************************************************)

Phase1 == IF Id1 \in DOMAIN member_materialization THEN Some(member_materialization[Id1]) ELSE None
Phase1Next == IF Id1 \in DOMAIN member_materialization' THEN Some(member_materialization'[Id1]) ELSE None

\* Wake run: a sealed plan leaves the member Dormant.
AuditWitnessSealedDormant == boot_plan_sealed /\ Phase1 = Some("Dormant")
\* Wake run: a sealed member with no endpoint is owed EndpointPending.
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
    /\ \E rid \in {Some(Rid1), None}, fence \in {Some(1), None}, ep \in AuditEpochs : ReleaseArm(rid, fence, ep)
\* Wake run: a Stop closes an open attempt.
AuditWitnessStopClosesStep == StopRunning /\ member_wake_attempts # [x \in {} |-> None]
\* Wake run: late feedback for an attempt a Stop closed is a stale no-op in Stopped.
AuditWitnessStaleInStopped ==
    /\ phase = "Stopped"
    /\ member_wake_attempt_sequence > 0
    /\ ENABLED ResolveMemberRevivalBoundStaleStopped(Id1, member_wake_attempt_sequence)
\* Wake run: the drift arm reopens a Live member's wake.
AuditWitnessDrift == member_materialization_drift_count > 0
\* Wake run: a retirement start closes an open attempt.
AuditWitnessRetireClosesStep ==
    /\ Phase1 = Some("Waking")
    /\ WRetire
\* Wake run: a wake verdict records Broken.
AuditWitnessBrokenWake == Id1 \in DOMAIN member_restore_failures /\ Id1 \in DOMAIN identity_to_runtime

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

NotAuditWitnessSealedDormant == ~AuditWitnessSealedDormant
NotAuditWitnessSealedEndpointPending == ~AuditWitnessSealedEndpointPending
NotAuditWitnessWokeLive == [][~AuditWitnessWokeLiveStep]_vars
NotAuditWitnessReleased == [][~AuditWitnessReleasedStep]_vars
NotAuditWitnessStopCloses == [][~AuditWitnessStopClosesStep]_vars
NotAuditWitnessStaleInStopped == ~AuditWitnessStaleInStopped
NotAuditWitnessDrift == ~AuditWitnessDrift
NotAuditWitnessRetireCloses == [][~AuditWitnessRetireClosesStep]_vars
NotAuditWitnessBrokenWake == ~AuditWitnessBrokenWake
NotAuditWitnessResumeJoins == [][~AuditWitnessResumeJoinsStep]_vars
NotAuditWitnessResumeReservesLive == [][~AuditWitnessResumeReservesLiveStep]_vars
NotAuditWitnessResumeFinishedLive == [][~AuditWitnessResumeFinishedLiveStep]_vars
NotAuditWitnessHeldEdge == ~AuditWitnessHeldEdge
NotAuditWitnessBothSeated == ~AuditWitnessBothSeated
NotAuditWitnessSeatEndpointPending == ~AuditWitnessSeatEndpointPending
NotAuditWitnessRotated == [][~AuditWitnessRotatedStep]_vars
NotAuditWitnessReleasedSpawn == [][~AuditWitnessReleasedSpawnStep]_vars
====
