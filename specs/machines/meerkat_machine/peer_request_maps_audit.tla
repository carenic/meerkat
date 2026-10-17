---- MODULE peer_request_maps_audit ----
\* Hand-written bounded audit of the two peer request identity maps:
\* pending_peer_request_addressees (the peer each outbound request was sent
\* to: a response to it is accepted only from that peer) and
\* inbound_peer_request_requesters (the peer each inbound request came from:
\* a response to it goes only to that peer).
\*
\* The generated ci.cfg sweep sets PeerCorrelationIdValues and PeerIdValues
\* to {}, so it never records a peer request and both maps' guards and
\* invariants hold vacuously there. This audit keeps the generated
\* MeerkatMachine model unchanged and explores, for one registered session,
\* one correlation id and two peers, in the Idle and Attached phases: every
\* arm that records, advances, answers, rejects, times out or fails an
\* outbound request, every arm that records or answers an inbound request,
\* and both read-only verdict signals, under every generated invariant
\* (including pending_peer_request_addressees_match_requests and
\* inbound_peer_request_requesters_match_requests) plus:
\*
\*   AuditVerdictsTotal (invariant): in the registered session, for the id
\*     and each peer, exactly one response-sender verdict and exactly one
\*     response-addressee verdict is enabled: the machine always answers.
\*   AuditResponsesOnlyFromAddressee (action property): a progress, terminal
\*     or rejection step for an outbound request is taken only for the peer
\*     recorded as its addressee.
\*   AuditSenderVerdictExact (action property): the response-sender verdict
\*     is Addressee exactly for the recorded addressee, NotAddressee for any
\*     other peer of a pending request, and UnknownRequest only when no
\*     request is pending under the id.
\*   AuditAddresseeVerdictExact (action property): the response-addressee
\*     verdict is Requester exactly for the recorded requester, NotRequester
\*     for any other peer, and UnknownRequest only when no inbound request
\*     is held under the id.
\*
\* AuditWitness* operators are reachability goals the runner checks as
\* expected violations, so the properties are not vacuous. The runner's
\* --mutants mode also checks that seeded model defects are refused: a
\* terminal response that ignores its responder, swapped sender verdicts,
\* swapped addressee verdicts, a send that records no addressee and a
\* receipt that records no requester.
\*
\* The TLC config is DERIVED from the generated ci.cfg on every run by
\* peer_request_maps_audit.sh. Run a deeper bound by hand with:
\*   specs/machines/meerkat_machine/peer_request_maps_audit.sh 12
EXTENDS model

CONSTANT AuditMaxSteps

VARIABLES
    audit_answered,              \* a terminal response from the addressee ended a request
    audit_progressed,            \* the addressee's progress was applied
    audit_answered_after_progress, \* a terminal response followed that progress
    audit_not_addressee,         \* a NotAddressee verdict was given
    audit_unknown_after_answer,  \* an UnknownRequest sender verdict followed an answer
    audit_replied,               \* an inbound request was replied to
    audit_not_requester          \* a NotRequester verdict was given

auditVars == <<audit_answered, audit_progressed, audit_answered_after_progress,
               audit_not_addressee, audit_unknown_after_answer, audit_replied,
               audit_not_requester>>

AuditSession == "sessionid_1"
AuditCorr == "corr_1"
AuditPeers == {"peer_1", "peer_2"}
AuditLanes == {"Queue", "Steer"}
AuditPrefixLength == 2

AuditPrefix ==
    \/ model_step_count = 0 /\ Initialize
       /\ UNCHANGED auditVars
    \/ model_step_count = 1 /\ RegisterSessionIdle(AuditSession, None, {})
       /\ UNCHANGED auditVars

Sent(c, a) == PeerRequestSentIdle(c, a) \/ PeerRequestSentAttached(c, a)
Progress(c, r) == PeerResponseProgressArrivedIdle(c, r) \/ PeerResponseProgressArrivedAttached(c, r)
Terminal(c, r) ==
    \/ PeerResponseTerminalArrivedCompletedIdle(c, "Completed", r)
    \/ PeerResponseTerminalArrivedCompletedAttached(c, "Completed", r)
    \/ PeerResponseTerminalArrivedFailedIdle(c, "Failed", r)
    \/ PeerResponseTerminalArrivedFailedAttached(c, "Failed", r)
Rejected(c, r) == PeerResponseRejectedIdle(c, r) \/ PeerResponseRejectedAttached(c, r)
TimedOut(c) == PeerRequestTimedOutIdle(c) \/ PeerRequestTimedOutAttached(c)
SendFailed(c) == PeerRequestSendFailedIdle(c) \/ PeerRequestSendFailedAttached(c)
SenderAddressee(c, r) ==
    ResolvePeerResponseSenderAddresseeIdle(c, r) \/ ResolvePeerResponseSenderAddresseeAttached(c, r)
SenderNotAddressee(c, r) ==
    ResolvePeerResponseSenderNotAddresseeIdle(c, r) \/ ResolvePeerResponseSenderNotAddresseeAttached(c, r)
SenderUnknown(c, r) ==
    ResolvePeerResponseSenderUnknownRequestIdle(c, r) \/ ResolvePeerResponseSenderUnknownRequestAttached(c, r)
Received(c, l, q) == PeerRequestReceivedIdle(c, l, q) \/ PeerRequestReceivedAttached(c, l, q)
Replied(c) == PeerResponseRepliedIdle(c) \/ PeerResponseRepliedAttached(c)
AddresseeRequester(c, a) ==
    ResolvePeerResponseAddresseeRequesterIdle(c, a) \/ ResolvePeerResponseAddresseeRequesterAttached(c, a)
AddresseeNotRequester(c, a) ==
    ResolvePeerResponseAddresseeNotRequesterIdle(c, a) \/ ResolvePeerResponseAddresseeNotRequesterAttached(c, a)
AddresseeUnknown(c, a) ==
    ResolvePeerResponseAddresseeUnknownRequestIdle(c, a) \/ ResolvePeerResponseAddresseeUnknownRequestAttached(c, a)

KeepHistory == UNCHANGED auditVars

AuditBody ==
    \/ EnsureSessionWithExecutorIdle(AuditSession) /\ KeepHistory
    \/ \E a \in AuditPeers : Sent(AuditCorr, a) /\ KeepHistory
    \/ (\E r \in AuditPeers : Progress(AuditCorr, r))
       /\ audit_progressed' = TRUE
       /\ UNCHANGED <<audit_answered, audit_answered_after_progress, audit_not_addressee,
                      audit_unknown_after_answer, audit_replied, audit_not_requester>>
    \/ (\E r \in AuditPeers : Terminal(AuditCorr, r))
       /\ audit_answered' = TRUE
       /\ audit_answered_after_progress' = (audit_answered_after_progress \/ audit_progressed)
       /\ UNCHANGED <<audit_progressed, audit_not_addressee, audit_unknown_after_answer,
                      audit_replied, audit_not_requester>>
    \/ (\E r \in AuditPeers : Rejected(AuditCorr, r)) /\ KeepHistory
    \/ (TimedOut(AuditCorr) \/ SendFailed(AuditCorr)) /\ KeepHistory
    \/ (\E r \in AuditPeers : SenderAddressee(AuditCorr, r)) /\ KeepHistory
    \/ (\E r \in AuditPeers : SenderNotAddressee(AuditCorr, r))
       /\ audit_not_addressee' = TRUE
       /\ UNCHANGED <<audit_answered, audit_progressed, audit_answered_after_progress,
                      audit_unknown_after_answer, audit_replied, audit_not_requester>>
    \/ (\E r \in AuditPeers : SenderUnknown(AuditCorr, r))
       /\ audit_unknown_after_answer' = (audit_unknown_after_answer \/ audit_answered)
       /\ UNCHANGED <<audit_answered, audit_progressed, audit_answered_after_progress,
                      audit_not_addressee, audit_replied, audit_not_requester>>
    \/ (\E l \in AuditLanes, q \in AuditPeers : Received(AuditCorr, l, q)) /\ KeepHistory
    \/ Replied(AuditCorr)
       /\ audit_replied' = TRUE
       /\ UNCHANGED <<audit_answered, audit_progressed, audit_answered_after_progress,
                      audit_not_addressee, audit_unknown_after_answer, audit_not_requester>>
    \/ (\E a \in AuditPeers : AddresseeRequester(AuditCorr, a) \/ AddresseeUnknown(AuditCorr, a))
       /\ KeepHistory
    \/ (\E a \in AuditPeers : AddresseeNotRequester(AuditCorr, a))
       /\ audit_not_requester' = TRUE
       /\ UNCHANGED <<audit_answered, audit_progressed, audit_answered_after_progress,
                      audit_not_addressee, audit_unknown_after_answer, audit_replied>>

AuditNext ==
    \/ AuditPrefix
    \/ (model_step_count >= AuditPrefixLength /\ AuditBody)

AuditInit ==
    /\ Init
    /\ audit_answered = FALSE
    /\ audit_progressed = FALSE
    /\ audit_answered_after_progress = FALSE
    /\ audit_not_addressee = FALSE
    /\ audit_unknown_after_answer = FALSE
    /\ audit_replied = FALSE
    /\ audit_not_requester = FALSE

AuditSpec == AuditInit /\ [][AuditNext]_<<vars, auditVars>>

AuditStateConstraint == model_step_count <= AuditMaxSteps

\* Exactly one of three conditions holds.
ExactlyOne(a, b, c) == (a \/ b \/ c) /\ ~(a /\ b) /\ ~(a /\ c) /\ ~(b /\ c)

AuditVerdictsTotal ==
    (session_id = Some(AuditSession) /\ phase \in {"Idle", "Attached"})
        => \A p \in AuditPeers :
            /\ ExactlyOne(ENABLED SenderAddressee(AuditCorr, p),
                          ENABLED SenderNotAddressee(AuditCorr, p),
                          ENABLED SenderUnknown(AuditCorr, p))
            /\ ExactlyOne(ENABLED AddresseeRequester(AuditCorr, p),
                          ENABLED AddresseeNotRequester(AuditCorr, p),
                          ENABLED AddresseeUnknown(AuditCorr, p))

RecordedAddressee(r) ==
    AuditCorr \in DOMAIN pending_peer_request_addressees
    /\ pending_peer_request_addressees[AuditCorr] = r

RecordedRequester(a) ==
    AuditCorr \in DOMAIN inbound_peer_request_requesters
    /\ inbound_peer_request_requesters[AuditCorr] = a

AuditResponsesOnlyFromAddressee ==
    [][\A r \in AuditPeers :
         (Progress(AuditCorr, r) \/ Terminal(AuditCorr, r) \/ Rejected(AuditCorr, r))
             => RecordedAddressee(r)]_<<vars, auditVars>>

AuditSenderVerdictExact ==
    [][\A r \in AuditPeers :
         /\ SenderAddressee(AuditCorr, r) => RecordedAddressee(r)
         /\ SenderNotAddressee(AuditCorr, r)
                => (AuditCorr \in DOMAIN pending_peer_requests /\ ~RecordedAddressee(r))
         /\ SenderUnknown(AuditCorr, r) => AuditCorr \notin DOMAIN pending_peer_requests]_<<vars, auditVars>>

AuditAddresseeVerdictExact ==
    [][\A a \in AuditPeers :
         /\ AddresseeRequester(AuditCorr, a) => RecordedRequester(a)
         /\ AddresseeNotRequester(AuditCorr, a)
                => (AuditCorr \in DOMAIN inbound_peer_requests /\ ~RecordedRequester(a))
         /\ AddresseeUnknown(AuditCorr, a) => AuditCorr \notin DOMAIN inbound_peer_requests]_<<vars, auditVars>>

\* Reachability goals, each checked as an expected violation of its negation.
\* The addressee's terminal response ended a request: both maps are empty.
AuditWitnessAnswered ==
    audit_answered /\ DOMAIN pending_peer_requests = {}
    /\ DOMAIN pending_peer_request_addressees = {}
\* The addressee's progress, then its terminal response.
AuditWitnessProgressThenAnswered == audit_answered_after_progress
\* Another peer's response to a pending request is judged NotAddressee.
AuditWitnessNotAddressee == audit_not_addressee
\* After the request ended, a late response is judged UnknownRequest.
AuditWitnessUnknownAfterAnswer == audit_unknown_after_answer
\* An inbound request was replied to: its requester record is gone.
AuditWitnessReplied ==
    audit_replied /\ DOMAIN inbound_peer_requests = {}
    /\ DOMAIN inbound_peer_request_requesters = {}
\* A response to an inbound request addressed to another peer is judged
\* NotRequester.
AuditWitnessNotRequester == audit_not_requester

NotAuditWitnessAnswered == ~AuditWitnessAnswered
NotAuditWitnessProgressThenAnswered == ~AuditWitnessProgressThenAnswered
NotAuditWitnessNotAddressee == ~AuditWitnessNotAddressee
NotAuditWitnessUnknownAfterAnswer == ~AuditWitnessUnknownAfterAnswer
NotAuditWitnessReplied == ~AuditWitnessReplied
NotAuditWitnessNotRequester == ~AuditWitnessNotRequester
====
