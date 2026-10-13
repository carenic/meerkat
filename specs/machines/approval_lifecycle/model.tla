---- MODULE model ----
EXTENDS TLC, Naturals, Sequences, FiniteSets

\* Generated semantic machine model for ApprovalLifecycleMachine.

CONSTANTS ApprovalLifecycleDecisionValues, ApprovalLifecycleRejectionReasonValues, ApprovalLifecycleStatusValues, BooleanValues, ReviewAttemptStatusValues, ReviewRetirementReasonValues, ReviewVerdictValues, SetOfStringValues, StringValues

None == [tag |-> "none", value |-> "none"]
Some(v) == [tag |-> "some", value |-> v]

MapStringApprovalLifecycleStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in ApprovalLifecycleStatusValues }
MapStringBoolValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in BOOLEAN }
MapStringReviewAttemptStatusValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in ReviewAttemptStatusValues }
MapStringReviewRetirementReasonValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in ReviewRetirementReasonValues }
MapStringStringValues == {[x \in {} |-> None]} \cup { [x \in {k} |-> v] : k \in StringValues, v \in StringValues }
OptionApprovalLifecycleDecisionValues == {None} \cup {Some(x) : x \in ApprovalLifecycleDecisionValues}

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

VARIABLES phase, model_step_count, approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements

vars == << phase, model_step_count, approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements >>

is_terminal_status(status) == (IF (status = "Approved") THEN TRUE ELSE (IF (status = "Denied") THEN TRUE ELSE (status = "Cancelled")))
allowed_non_empty(approve_allowed, deny_allowed) == (IF approve_allowed THEN TRUE ELSE deny_allowed)
restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) == (allowed_non_empty(approve_allowed, deny_allowed) /\ (IF ((IF (status = "Pending") THEN TRUE ELSE (IF (status = "Expired") THEN TRUE ELSE (status = "Cancelled"))) /\ (decision = None)) THEN TRUE ELSE (IF ((status = "Approved") /\ approve_allowed /\ (decision = Some("Approve"))) THEN TRUE ELSE ((status = "Denied") /\ deny_allowed /\ (decision = Some("Deny"))))) /\ (IF (authenticated = FALSE) THEN TRUE ELSE consent) /\ (IF (consent = FALSE) THEN TRUE ELSE (IF ((IF (status = "Approved") THEN TRUE ELSE (status = "Denied")) /\ authenticated) THEN TRUE ELSE ((status # "Approved") /\ (status # "Denied") /\ (authenticated = FALSE)))) /\ (IF (consumed = FALSE) THEN TRUE ELSE (authenticated /\ (status = "Approved"))))

Init ==
    /\ phase = "Ready"
    /\ model_step_count = 0
    /\ approval_ids = {}
    /\ approval_statuses = [x \in {} |-> None]
    /\ approval_approve_allowed = [x \in {} |-> None]
    /\ approval_deny_allowed = [x \in {} |-> None]
    /\ approval_has_expiry = [x \in {} |-> None]
    /\ approval_consent = [x \in {} |-> None]
    /\ approval_consent_authenticated = [x \in {} |-> None]
    /\ approval_consent_consumed = [x \in {} |-> None]
    /\ approval_consent_review = [x \in {} |-> None]
    /\ review_consent_approval = [x \in {} |-> None]
    /\ review_ids = {}
    /\ review_statuses = [x \in {} |-> None]
    /\ review_retirements = [x \in {} |-> None]

\* Named UNCHANGED frames. One definition per distinct frame; every action
\* that leaves those variables unchanged references the definition by name.
UnchangedFrame_2d1a5b23a3815a3f == UNCHANGED << approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements >>
UnchangedFrame_2fb11a9a690d4843 == UNCHANGED << review_retirements >>
UnchangedFrame_33113be9e303398d == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, review_ids >>
UnchangedFrame_34e6ec4e027ac862 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval >>
UnchangedFrame_47563bb318a26a61 == UNCHANGED << approval_ids, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements >>
UnchangedFrame_4c5d7644ec35cebf == UNCHANGED << approval_ids, approval_consent, approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements >>
UnchangedFrame_6005d1854c06fb88 == UNCHANGED << approval_ids, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_consumed, review_ids, review_retirements >>
UnchangedFrame_6ba07ea796f5d1c0 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_retirements >>
UnchangedFrame_8d0196a9b04a6297 == UNCHANGED << approval_ids, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, review_ids >>
UnchangedFrame_b33e46d17599a683 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_retirements >>
UnchangedFrame_b78ed52c0aa5c659 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, review_ids, review_retirements >>
UnchangedFrame_cee9d07ebc263907 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_statuses, review_retirements >>
UnchangedFrame_f16b332b9be4bd60 == UNCHANGED << approval_ids, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids, review_retirements >>
UnchangedFrame_f9fca277a7a0b725 == UNCHANGED << approval_ids, approval_statuses, approval_approve_allowed, approval_deny_allowed, approval_has_expiry, approval_consent, approval_consent_authenticated, approval_consent_consumed, approval_consent_review, review_consent_approval, review_ids >>

CreateRejectedEmptyAllowedDecisions(approval_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


CreateRejectedAlreadyExists(approval_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) /\ (approval_id \in approval_ids))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


CreatePending(approval_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) /\ ((approval_id \in approval_ids) = FALSE))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Pending")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, FALSE)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, FALSE)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, FALSE)
    /\ UnchangedFrame_2d1a5b23a3815a3f


CreateConsentRejectedEmptyAllowedDecisions(approval_id, review_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


CreateConsentRejectedAlreadyExists(approval_id, review_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) /\ (IF (approval_id \in approval_ids) THEN TRUE ELSE (review_id \in review_ids)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


CreateConsentPending(approval_id, review_id, approve_allowed, deny_allowed, has_expiry) ==
    /\ phase = "Ready"
    /\ (allowed_non_empty(approve_allowed, deny_allowed) /\ ((approval_id \in approval_ids) = FALSE) /\ ((review_id \in review_ids) = FALSE))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Pending")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, TRUE)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, FALSE)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, FALSE)
    /\ approval_consent_review' = MapSet(approval_consent_review, approval_id, review_id)
    /\ review_consent_approval' = MapSet(review_consent_approval, review_id, approval_id)
    /\ review_ids' = (review_ids \cup {review_id})
    /\ review_statuses' = MapSet(review_statuses, review_id, "Pending")
    /\ UnchangedFrame_2fb11a9a690d4843


RestoreRejectedDuplicate(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (approval_id \in approval_ids)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RestoreRejectedEmptyAllowedDecisions(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ (allowed_non_empty(approve_allowed, deny_allowed) = FALSE))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RestorePending(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (status = "Pending") /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Pending")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, consent)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_2d1a5b23a3815a3f


RestoreExpired(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (status = "Expired") /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Expired")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, consent)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_2d1a5b23a3815a3f


RestoreCancelled(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (status = "Cancelled") /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Cancelled")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, consent)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_2d1a5b23a3815a3f


RestoreApproved(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (status = "Approved") /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Approved")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, consent)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_2d1a5b23a3815a3f


RestoreDenied(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (status = "Denied") /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_ids' = (approval_ids \cup {approval_id})
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Denied")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent' = MapSet(approval_consent, approval_id, consent)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_2d1a5b23a3815a3f


RestoreRejectedInvalidRecord(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ (((approval_id \in approval_ids) = FALSE) /\ allowed_non_empty(approve_allowed, deny_allowed) /\ (restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) = FALSE))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReconcileRejectedMissing(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReconcileRejectedLinked(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) /\ (approval_id \in DOMAIN approval_consent_review))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReconcileRejectedInvalidRecord(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF (restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) = FALSE) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ((approval_consent)[approval_id] # consent) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed)))))) /\ ((approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ (IF (restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) = FALSE) THEN TRUE ELSE (IF ((approval_consent)[approval_id] # consent) THEN TRUE ELSE ((approval_consent_consumed)[approval_id] /\ (consumed = FALSE))))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReconcileAppliedPending(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((status = "Pending")) THEN TRUE ELSE (IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF ~(restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = consent)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))))) /\ ((status = "Pending") /\ (approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) /\ ((approval_consent)[approval_id] = consent) /\ (IF ((approval_consent_consumed)[approval_id] = FALSE) THEN TRUE ELSE consumed)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Pending")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_4c5d7644ec35cebf


ReconcileAppliedExpired(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((status = "Expired")) THEN TRUE ELSE (IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF ~(restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = consent)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))))) /\ ((status = "Expired") /\ (approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) /\ ((approval_consent)[approval_id] = consent) /\ (IF ((approval_consent_consumed)[approval_id] = FALSE) THEN TRUE ELSE consumed)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Expired")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_4c5d7644ec35cebf


ReconcileAppliedCancelled(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((status = "Cancelled")) THEN TRUE ELSE (IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF ~(restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = consent)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))))) /\ ((status = "Cancelled") /\ (approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) /\ ((approval_consent)[approval_id] = consent) /\ (IF ((approval_consent_consumed)[approval_id] = FALSE) THEN TRUE ELSE consumed)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Cancelled")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_4c5d7644ec35cebf


ReconcileAppliedApproved(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((status = "Approved")) THEN TRUE ELSE (IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF ~(restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = consent)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))))) /\ ((status = "Approved") /\ (approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) /\ ((approval_consent)[approval_id] = consent) /\ (IF ((approval_consent_consumed)[approval_id] = FALSE) THEN TRUE ELSE consumed)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Approved")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_4c5d7644ec35cebf


ReconcileAppliedDenied(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed) ==
    /\ phase = "Ready"
    /\ ((IF ~((status = "Denied")) THEN TRUE ELSE (IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (IF ~(restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = consent)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))))) /\ ((status = "Denied") /\ (approval_id \in approval_ids) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ restored_record_valid(status, approve_allowed, deny_allowed, decision, consent, authenticated, consumed) /\ ((approval_consent)[approval_id] = consent) /\ (IF ((approval_consent_consumed)[approval_id] = FALSE) THEN TRUE ELSE consumed)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Denied")
    /\ approval_approve_allowed' = MapSet(approval_approve_allowed, approval_id, approve_allowed)
    /\ approval_deny_allowed' = MapSet(approval_deny_allowed, approval_id, deny_allowed)
    /\ approval_has_expiry' = MapSet(approval_has_expiry, approval_id, has_expiry)
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, authenticated)
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, consumed)
    /\ UnchangedFrame_4c5d7644ec35cebf


ObserveExpiryRejectedMissing(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ObserveExpiryExpiresPending(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (approval_id \in DOMAIN approval_has_expiry)))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ (approval_has_expiry)[approval_id] /\ expired))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Expired")
    /\ UnchangedFrame_47563bb318a26a61


ObserveExpiryPendingNoop(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (approval_id \in DOMAIN approval_has_expiry)))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ (IF ((approval_has_expiry)[approval_id] = FALSE) THEN TRUE ELSE (expired = FALSE))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ObserveExpiryApprovedNoop(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Approved")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ObserveExpiryDeniedNoop(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Denied")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ObserveExpiryExpiredNoop(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Expired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ObserveExpiryCancelledNoop(approval_id, expired) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Cancelled")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedMissing(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedExpired(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Expired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedAlreadyDecided(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses)) /\ ((approval_id \in approval_ids) /\ is_terminal_status((approval_statuses)[approval_id])))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedAuthenticationRequired(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent)))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ (approval_consent)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedApproveNotAllowed(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = FALSE)) THEN TRUE ELSE (IF ~((decision = "Approve")) THEN TRUE ELSE (approval_id \in DOMAIN approval_approve_allowed))))))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ ((approval_consent)[approval_id] = FALSE) /\ (decision = "Approve") /\ ((approval_approve_allowed)[approval_id] = FALSE)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideRejectedDenyNotAllowed(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = FALSE)) THEN TRUE ELSE (IF ~((decision = "Deny")) THEN TRUE ELSE (approval_id \in DOMAIN approval_deny_allowed))))))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ ((approval_consent)[approval_id] = FALSE) /\ (decision = "Deny") /\ ((approval_deny_allowed)[approval_id] = FALSE)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideApprove(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = FALSE)) THEN TRUE ELSE (IF ~((decision = "Approve")) THEN TRUE ELSE (approval_id \in DOMAIN approval_approve_allowed))))))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ ((approval_consent)[approval_id] = FALSE) /\ (decision = "Approve") /\ (approval_approve_allowed)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Approved")
    /\ UnchangedFrame_47563bb318a26a61


DecideDeny(approval_id, decision) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = FALSE)) THEN TRUE ELSE (IF ~((decision = "Deny")) THEN TRUE ELSE (approval_id \in DOMAIN approval_deny_allowed))))))) /\ ((approval_id \in approval_ids) /\ ((approval_statuses)[approval_id] = "Pending") /\ ((approval_consent)[approval_id] = FALSE) /\ (decision = "Deny") /\ (approval_deny_allowed)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Denied")
    /\ UnchangedFrame_47563bb318a26a61


DecideConsentRejectedMissing(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentRejectedMismatch(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review))) /\ ((approval_id \in approval_ids) /\ (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE ((approval_consent_review)[approval_id] # review_id))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentRejectedExpired(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Expired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentRejectedAlreadyDecided(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ is_terminal_status((approval_statuses)[approval_id])))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentRejectedAuthenticationRequired(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Pending") /\ (authenticated = FALSE)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentRejectedNotAllowed(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (IF ~(authenticated) THEN TRUE ELSE ((IF ~((decision = "Approve")) THEN TRUE ELSE (approval_id \in DOMAIN approval_approve_allowed)) /\ (IF ((decision = "Approve") /\ ((approval_approve_allowed)[approval_id] = FALSE)) THEN TRUE ELSE (IF ~((decision = "Deny")) THEN TRUE ELSE (approval_id \in DOMAIN approval_deny_allowed))))))))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Pending") /\ authenticated /\ (IF ((decision = "Approve") /\ ((approval_approve_allowed)[approval_id] = FALSE)) THEN TRUE ELSE ((decision = "Deny") /\ ((approval_deny_allowed)[approval_id] = FALSE)))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


DecideConsentApprove(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (IF ~(authenticated) THEN TRUE ELSE (IF ~((decision = "Approve")) THEN TRUE ELSE (approval_id \in DOMAIN approval_approve_allowed))))))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Pending") /\ authenticated /\ (decision = "Approve") /\ (approval_approve_allowed)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Approved")
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, TRUE)
    /\ review_statuses' = MapSet(review_statuses, review_id, "Allowed")
    /\ UnchangedFrame_f16b332b9be4bd60


DecideConsentDeny(approval_id, review_id, decision, authenticated) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Pending")) THEN TRUE ELSE (IF ~(authenticated) THEN TRUE ELSE (IF ~((decision = "Deny")) THEN TRUE ELSE (approval_id \in DOMAIN approval_deny_allowed))))))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Pending") /\ authenticated /\ (decision = "Deny") /\ (approval_deny_allowed)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Denied")
    /\ approval_consent_authenticated' = MapSet(approval_consent_authenticated, approval_id, TRUE)
    /\ approval_consent_review' = MapRemove(approval_consent_review, approval_id)
    /\ review_consent_approval' = MapRemove(review_consent_approval, review_id)
    /\ review_statuses' = MapSet(review_statuses, review_id, "Denied")
    /\ UnchangedFrame_6005d1854c06fb88


ConsumeConsentRejectedMissing(approval_id, review_id) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeConsentRejectedAlreadyConsumed(approval_id, review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed)) /\ ((approval_id \in approval_ids) /\ (approval_consent_consumed)[approval_id]))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeConsentRejectedMismatch(approval_id, review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE ((approval_consent_review)[approval_id] # review_id))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeConsentRejectedNotSatisfied(approval_id, review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE ((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ((approval_statuses)[approval_id] # "Approved") THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_authenticated) /\ (IF ((approval_consent_authenticated)[approval_id] = FALSE) THEN TRUE ELSE (review_id \in DOMAIN review_statuses)))))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ ((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ (IF ((approval_statuses)[approval_id] # "Approved") THEN TRUE ELSE (IF ((approval_consent_authenticated)[approval_id] = FALSE) THEN TRUE ELSE ((review_statuses)[review_id] # "Allowed")))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeConsentForEntry(approval_id, review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE ((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ~(((approval_statuses)[approval_id] = "Approved")) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_authenticated) /\ (IF ~((approval_consent_authenticated)[approval_id]) THEN TRUE ELSE (review_id \in DOMAIN review_statuses)))))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ ((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Approved") /\ (approval_consent_authenticated)[approval_id] /\ ((review_statuses)[review_id] = "Allowed")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_consent_consumed' = MapSet(approval_consent_consumed, approval_id, TRUE)
    /\ approval_consent_review' = MapRemove(approval_consent_review, approval_id)
    /\ review_consent_approval' = MapRemove(review_consent_approval, review_id)
    /\ review_statuses' = MapSet(review_statuses, review_id, "Used")
    /\ UnchangedFrame_b78ed52c0aa5c659


WithdrawConsentRejectedMissing(approval_id, review_id, reason) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


WithdrawConsentRejectedMismatch(approval_id, review_id, reason) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review))) /\ ((approval_id \in approval_ids) /\ (IF ((approval_id \in DOMAIN approval_consent_review) = FALSE) THEN TRUE ELSE ((approval_consent_review)[approval_id] # review_id))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


WithdrawConsentPending(approval_id, review_id, reason) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] = "Pending")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Cancelled")
    /\ approval_consent_review' = MapRemove(approval_consent_review, approval_id)
    /\ review_consent_approval' = MapRemove(review_consent_approval, review_id)
    /\ review_statuses' = MapSet(review_statuses, review_id, "Retired")
    /\ review_retirements' = MapSet(review_retirements, review_id, reason)
    /\ UnchangedFrame_8d0196a9b04a6297


WithdrawConsentSettled(approval_id, review_id, reason) ==
    /\ phase = "Ready"
    /\ (((IF ~((approval_id \in DOMAIN approval_consent_review)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_review)) /\ (IF ~(((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id))) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))) /\ (((approval_id \in DOMAIN approval_consent_review) /\ ((approval_consent_review)[approval_id] = review_id)) /\ ((approval_statuses)[approval_id] # "Pending")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_consent_review' = MapRemove(approval_consent_review, approval_id)
    /\ review_consent_approval' = MapRemove(review_consent_approval, review_id)
    /\ review_statuses' = MapSet(review_statuses, review_id, "Retired")
    /\ review_retirements' = MapSet(review_retirements, review_id, reason)
    /\ UnchangedFrame_33113be9e303398d


SettleUnlinkedRejectedMissing(approval_id) ==
    /\ phase = "Ready"
    /\ ((approval_id \in approval_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedRejectedNotConsent(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent)) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = FALSE)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedRejectedLinked(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent)) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ (approval_id \in DOMAIN approval_consent_review)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedRejectedConsumed(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = TRUE)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE (approval_id \in DOMAIN approval_consent_consumed))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ ((approval_consent_consumed)[approval_id] = TRUE)))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedRejectedExpired(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = TRUE)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ ((approval_statuses)[approval_id] = "Expired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedRejectedSettled(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = TRUE)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_statuses) /\ (IF ((approval_statuses)[approval_id] = "Denied") THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ (IF ((approval_statuses)[approval_id] = "Denied") THEN TRUE ELSE ((approval_statuses)[approval_id] = "Cancelled"))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


SettleUnlinkedPending(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = TRUE)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ ((approval_statuses)[approval_id] = "Pending")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ approval_statuses' = MapSet(approval_statuses, approval_id, "Cancelled")
    /\ UnchangedFrame_47563bb318a26a61


SettleUnlinkedApproved(approval_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((approval_id \in approval_ids)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent) /\ (IF ~(((approval_consent)[approval_id] = TRUE)) THEN TRUE ELSE (IF ~(((approval_id \in DOMAIN approval_consent_review) = FALSE)) THEN TRUE ELSE ((approval_id \in DOMAIN approval_consent_consumed) /\ (IF ~(((approval_consent_consumed)[approval_id] = FALSE)) THEN TRUE ELSE (approval_id \in DOMAIN approval_statuses))))))) /\ ((approval_id \in approval_ids) /\ ((approval_consent)[approval_id] = TRUE) /\ ((approval_id \in DOMAIN approval_consent_review) = FALSE) /\ ((approval_consent_consumed)[approval_id] = FALSE) /\ ((approval_statuses)[approval_id] = "Approved")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


BeginReviewRejectedDuplicate(review_id) ==
    /\ phase = "Ready"
    /\ (review_id \in review_ids)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


BeginReviewPending(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \cup {review_id})
    /\ review_statuses' = MapSet(review_statuses, review_id, "Pending")
    /\ UnchangedFrame_b33e46d17599a683


RecordReviewVerdictRejectedMissing(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewVerdictRejectedConsentLinked(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) /\ (review_id \in DOMAIN review_consent_approval))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewVerdictRejectedRetired(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewVerdictRejectedSettled(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ~(((review_statuses)[review_id] # "Pending")) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] # "Pending") /\ ((review_statuses)[review_id] # "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewVerdictAllowed(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Pending") /\ (verdict = "Allow")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Allowed")
    /\ UnchangedFrame_6ba07ea796f5d1c0


RecordReviewVerdictDenied(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Pending") /\ (verdict = "Deny")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Denied")
    /\ UnchangedFrame_6ba07ea796f5d1c0


RecordReviewVerdictEscalated(review_id, verdict) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Pending") /\ (verdict = "Escalate")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Escalated")
    /\ UnchangedFrame_6ba07ea796f5d1c0


RecordReviewUnavailableRejectedMissing(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewUnavailableRejectedConsentLinked(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) /\ (review_id \in DOMAIN review_consent_approval))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewUnavailableRejectedRetired(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewUnavailableRejectedSettled(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ~(((review_statuses)[review_id] # "Pending")) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] # "Pending") /\ ((review_statuses)[review_id] # "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RecordReviewUnavailable(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Pending")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Unavailable")
    /\ UnchangedFrame_6ba07ea796f5d1c0


RetireReviewRejectedMissing(review_id, reason) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RetireReviewRejectedConsentLinked(review_id, reason) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) /\ (review_id \in DOMAIN review_consent_approval))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RetireReviewRejectedRetired(review_id, reason) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RetireReviewRejectedSettled(review_id, reason) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ~(((review_statuses)[review_id] # "Pending")) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ~(((review_statuses)[review_id] # "Allowed")) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))))))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] # "Pending") /\ ((review_statuses)[review_id] # "Allowed") /\ ((review_statuses)[review_id] # "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


RetireReview(review_id, reason) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ((review_statuses)[review_id] = "Pending") THEN TRUE ELSE (review_id \in DOMAIN review_statuses))))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ (IF ((review_statuses)[review_id] = "Pending") THEN TRUE ELSE ((review_statuses)[review_id] = "Allowed"))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Retired")
    /\ review_retirements' = MapSet(review_retirements, review_id, reason)
    /\ UnchangedFrame_f9fca277a7a0b725


ConsumeReviewRejectedMissing(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeReviewRejectedConsentLinked(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) /\ (review_id \in DOMAIN review_consent_approval))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeReviewRejectedRetired(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeReviewRejectedUsed(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Used")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeReviewRejectedNotSatisfied(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ((review_statuses)[review_id] = "Pending") THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ((review_statuses)[review_id] = "Denied") THEN TRUE ELSE ((review_id \in DOMAIN review_statuses) /\ (IF ((review_statuses)[review_id] = "Escalated") THEN TRUE ELSE (review_id \in DOMAIN review_statuses))))))))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ (IF ((review_statuses)[review_id] = "Pending") THEN TRUE ELSE (IF ((review_statuses)[review_id] = "Denied") THEN TRUE ELSE (IF ((review_statuses)[review_id] = "Escalated") THEN TRUE ELSE ((review_statuses)[review_id] = "Unavailable"))))))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ConsumeReviewForEntry(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Allowed")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_statuses' = MapSet(review_statuses, review_id, "Used")
    /\ UnchangedFrame_6ba07ea796f5d1c0


ReleaseReviewRejectedMissing(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) = FALSE)
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReleaseReviewRejectedConsentLinked(review_id) ==
    /\ phase = "Ready"
    /\ ((review_id \in review_ids) /\ (review_id \in DOMAIN review_consent_approval))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReleaseReviewRejectedPending(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Pending")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ UnchangedFrame_cee9d07ebc263907


ReleaseReviewAllowed(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Allowed")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


ReleaseReviewDenied(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Denied")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


ReleaseReviewEscalated(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Escalated")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


ReleaseReviewUnavailable(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Unavailable")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


ReleaseReviewRetired(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Retired")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


ReleaseReviewUsed(review_id) ==
    /\ phase = "Ready"
    /\ ((IF ~((review_id \in review_ids)) THEN TRUE ELSE (IF ~(((review_id \in DOMAIN review_consent_approval) = FALSE)) THEN TRUE ELSE (review_id \in DOMAIN review_statuses))) /\ ((review_id \in review_ids) /\ ((review_id \in DOMAIN review_consent_approval) = FALSE) /\ ((review_statuses)[review_id] = "Used")))
    /\ phase' = "Ready"
    /\ model_step_count' = model_step_count + 1
    /\ review_ids' = (review_ids \ {review_id})
    /\ review_statuses' = MapRemove(review_statuses, review_id)
    /\ review_retirements' = MapRemove(review_retirements, review_id)
    /\ UnchangedFrame_34e6ec4e027ac862


Next ==
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreateRejectedEmptyAllowedDecisions(approval_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreateRejectedAlreadyExists(approval_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreatePending(approval_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreateConsentRejectedEmptyAllowedDecisions(approval_id, review_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreateConsentRejectedAlreadyExists(approval_id, review_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : CreateConsentPending(approval_id, review_id, approve_allowed, deny_allowed, has_expiry)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreRejectedDuplicate(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreRejectedEmptyAllowedDecisions(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestorePending(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreExpired(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreCancelled(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreApproved(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreDenied(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : RestoreRejectedInvalidRecord(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileRejectedMissing(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileRejectedLinked(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileRejectedInvalidRecord(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileAppliedPending(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileAppliedExpired(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileAppliedCancelled(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileAppliedApproved(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E status \in ApprovalLifecycleStatusValues : \E approve_allowed \in BOOLEAN : \E deny_allowed \in BOOLEAN : \E has_expiry \in BOOLEAN : \E decision \in OptionApprovalLifecycleDecisionValues : \E consent \in BOOLEAN : \E authenticated \in BOOLEAN : \E consumed \in BOOLEAN : ReconcileAppliedDenied(approval_id, status, approve_allowed, deny_allowed, has_expiry, decision, consent, authenticated, consumed)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryRejectedMissing(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : ObserveExpiryExpiresPending(approval_id, TRUE)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryPendingNoop(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryApprovedNoop(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryDeniedNoop(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryExpiredNoop(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E expired \in BOOLEAN : ObserveExpiryCancelledNoop(approval_id, expired)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedMissing(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedExpired(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedAlreadyDecided(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedAuthenticationRequired(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedApproveNotAllowed(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideRejectedDenyNotAllowed(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideApprove(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideDeny(approval_id, decision)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : \E authenticated \in BOOLEAN : DecideConsentRejectedMissing(approval_id, review_id, decision, authenticated)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : \E authenticated \in BOOLEAN : DecideConsentRejectedMismatch(approval_id, review_id, decision, authenticated)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : \E authenticated \in BOOLEAN : DecideConsentRejectedExpired(approval_id, review_id, decision, authenticated)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : \E authenticated \in BOOLEAN : DecideConsentRejectedAlreadyDecided(approval_id, review_id, decision, authenticated)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideConsentRejectedAuthenticationRequired(approval_id, review_id, decision, FALSE)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideConsentRejectedNotAllowed(approval_id, review_id, decision, TRUE)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideConsentApprove(approval_id, review_id, decision, TRUE)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E decision \in ApprovalLifecycleDecisionValues : DecideConsentDeny(approval_id, review_id, decision, TRUE)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : ConsumeConsentRejectedMissing(approval_id, review_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : ConsumeConsentRejectedAlreadyConsumed(approval_id, review_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : ConsumeConsentRejectedMismatch(approval_id, review_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : ConsumeConsentRejectedNotSatisfied(approval_id, review_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : ConsumeConsentForEntry(approval_id, review_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : WithdrawConsentRejectedMissing(approval_id, review_id, reason)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : WithdrawConsentRejectedMismatch(approval_id, review_id, reason)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : WithdrawConsentPending(approval_id, review_id, reason)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : WithdrawConsentSettled(approval_id, review_id, reason)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedMissing(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedNotConsent(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedLinked(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedConsumed(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedExpired(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedRejectedSettled(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedPending(approval_id)
    \/ (phase = "Ready") /\ \E approval_id \in StringValues : SettleUnlinkedApproved(approval_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : BeginReviewRejectedDuplicate(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : BeginReviewPending(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictRejectedMissing(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictRejectedConsentLinked(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictRejectedRetired(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictRejectedSettled(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictAllowed(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictDenied(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E verdict \in ReviewVerdictValues : RecordReviewVerdictEscalated(review_id, verdict)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : RecordReviewUnavailableRejectedMissing(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : RecordReviewUnavailableRejectedConsentLinked(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : RecordReviewUnavailableRejectedRetired(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : RecordReviewUnavailableRejectedSettled(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : RecordReviewUnavailable(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : RetireReviewRejectedMissing(review_id, reason)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : RetireReviewRejectedConsentLinked(review_id, reason)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : RetireReviewRejectedRetired(review_id, reason)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : RetireReviewRejectedSettled(review_id, reason)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : \E reason \in ReviewRetirementReasonValues : RetireReview(review_id, reason)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewRejectedMissing(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewRejectedConsentLinked(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewRejectedRetired(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewRejectedUsed(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewRejectedNotSatisfied(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ConsumeReviewForEntry(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewRejectedMissing(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewRejectedConsentLinked(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewRejectedPending(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewAllowed(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewDenied(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewEscalated(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewUnavailable(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewRetired(review_id)
    \/ (phase = "Ready") /\ \E review_id \in StringValues : ReleaseReviewUsed(review_id)

approval_maps_cover_exactly_the_registered_ids == ((DOMAIN approval_statuses = approval_ids) /\ (DOMAIN approval_approve_allowed = approval_ids) /\ (DOMAIN approval_deny_allowed = approval_ids) /\ (DOMAIN approval_has_expiry = approval_ids))
review_statuses_cover_exactly_the_review_ids == (DOMAIN review_statuses = review_ids)
approval_consent_covers_exactly_the_approval_ids == ((DOMAIN approval_consent = approval_ids) /\ (DOMAIN approval_consent_authenticated = approval_ids) /\ (DOMAIN approval_consent_consumed = approval_ids))
review_retirement_only_for_retired_attempts == (\A id \in DOMAIN review_retirements : ((IF (id \in DOMAIN review_statuses) THEN Some((IF id \in DOMAIN review_statuses THEN review_statuses[id] ELSE "None")) ELSE None) = Some("Retired")))
consent_link_is_a_bijection_over_live_attempts == ((\A id \in DOMAIN approval_consent_review : (((IF (id \in DOMAIN approval_consent) THEN Some((IF id \in DOMAIN approval_consent THEN approval_consent[id] ELSE FALSE)) ELSE None) = Some(TRUE)) /\ ((IF ((IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_consent_approval) THEN Some((IF (IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_consent_approval THEN review_consent_approval[(IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None)] ELSE "None")) ELSE None) = Some(id)))) /\ (\A review \in DOMAIN review_consent_approval : (((IF ((IF "value" \in DOMAIN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None) THEN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN approval_consent_review) THEN Some((IF (IF "value" \in DOMAIN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None) THEN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN approval_consent_review THEN approval_consent_review[(IF "value" \in DOMAIN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None) THEN (IF (review \in DOMAIN review_consent_approval) THEN Some((IF review \in DOMAIN review_consent_approval THEN review_consent_approval[review] ELSE "None")) ELSE None)["value"] ELSE None)] ELSE "None")) ELSE None) = Some(review)) /\ (IF ((IF (review \in DOMAIN review_statuses) THEN Some((IF review \in DOMAIN review_statuses THEN review_statuses[review] ELSE "None")) ELSE None) = Some("Pending")) THEN TRUE ELSE ((IF (review \in DOMAIN review_statuses) THEN Some((IF review \in DOMAIN review_statuses THEN review_statuses[review] ELSE "None")) ELSE None) = Some("Allowed"))))))
linked_attempt_allowed_iff_approved == (\A id \in DOMAIN approval_consent_review : (IF (((IF (id \in DOMAIN approval_statuses) THEN Some((IF id \in DOMAIN approval_statuses THEN approval_statuses[id] ELSE "None")) ELSE None) = Some("Approved")) /\ ((IF ((IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_statuses) THEN Some((IF (IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_statuses THEN review_statuses[(IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None)] ELSE "None")) ELSE None) = Some("Allowed"))) THEN TRUE ELSE (((IF (id \in DOMAIN approval_statuses) THEN Some((IF id \in DOMAIN approval_statuses THEN approval_statuses[id] ELSE "None")) ELSE None) # Some("Approved")) /\ ((IF ((IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_statuses) THEN Some((IF (IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None) \in DOMAIN review_statuses THEN review_statuses[(IF "value" \in DOMAIN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None) THEN (IF (id \in DOMAIN approval_consent_review) THEN Some((IF id \in DOMAIN approval_consent_review THEN approval_consent_review[id] ELSE "None")) ELSE None)["value"] ELSE None)] ELSE "None")) ELSE None) = Some("Pending")))))
consumed_consent_is_approved_authenticated_and_unlinked == (\A id \in DOMAIN approval_consent_consumed : (IF ((IF (id \in DOMAIN approval_consent_consumed) THEN Some((IF id \in DOMAIN approval_consent_consumed THEN approval_consent_consumed[id] ELSE FALSE)) ELSE None) = Some(FALSE)) THEN TRUE ELSE (((IF (id \in DOMAIN approval_statuses) THEN Some((IF id \in DOMAIN approval_statuses THEN approval_statuses[id] ELSE "None")) ELSE None) = Some("Approved")) /\ ((IF (id \in DOMAIN approval_consent_authenticated) THEN Some((IF id \in DOMAIN approval_consent_authenticated THEN approval_consent_authenticated[id] ELSE FALSE)) ELSE None) = Some(TRUE)) /\ ((id \in DOMAIN approval_consent_review) = FALSE))))

CiStateConstraint == /\ model_step_count <= 6 /\ Cardinality(approval_ids) <= 1 /\ Cardinality(DOMAIN approval_statuses) <= 1 /\ Cardinality(DOMAIN approval_approve_allowed) <= 1 /\ Cardinality(DOMAIN approval_deny_allowed) <= 1 /\ Cardinality(DOMAIN approval_has_expiry) <= 1 /\ Cardinality(DOMAIN approval_consent) <= 1 /\ Cardinality(DOMAIN approval_consent_authenticated) <= 1 /\ Cardinality(DOMAIN approval_consent_consumed) <= 1 /\ Cardinality(DOMAIN approval_consent_review) <= 1 /\ Cardinality(DOMAIN review_consent_approval) <= 1 /\ Cardinality(review_ids) <= 1 /\ Cardinality(DOMAIN review_statuses) <= 1 /\ Cardinality(DOMAIN review_retirements) <= 1
DeepStateConstraint == /\ model_step_count <= 8 /\ Cardinality(approval_ids) <= 2 /\ Cardinality(DOMAIN approval_statuses) <= 2 /\ Cardinality(DOMAIN approval_approve_allowed) <= 2 /\ Cardinality(DOMAIN approval_deny_allowed) <= 2 /\ Cardinality(DOMAIN approval_has_expiry) <= 2 /\ Cardinality(DOMAIN approval_consent) <= 2 /\ Cardinality(DOMAIN approval_consent_authenticated) <= 2 /\ Cardinality(DOMAIN approval_consent_consumed) <= 2 /\ Cardinality(DOMAIN approval_consent_review) <= 2 /\ Cardinality(DOMAIN review_consent_approval) <= 2 /\ Cardinality(review_ids) <= 2 /\ Cardinality(DOMAIN review_statuses) <= 2 /\ Cardinality(DOMAIN review_retirements) <= 2

Spec == Init /\ [][Next]_vars

THEOREM Spec => []approval_maps_cover_exactly_the_registered_ids
THEOREM Spec => []review_statuses_cover_exactly_the_review_ids
THEOREM Spec => []approval_consent_covers_exactly_the_approval_ids
THEOREM Spec => []review_retirement_only_for_retired_attempts
THEOREM Spec => []consent_link_is_a_bijection_over_live_attempts
THEOREM Spec => []linked_attempt_allowed_iff_approved
THEOREM Spec => []consumed_consent_is_approved_authenticated_and_unlinked

=============================================================================
