"""Hand-written comms/send typing agrees with the generated wire contracts."""

from __future__ import annotations

import sys
from typing import get_args

from meerkat import MeerkatClient
from meerkat.client import _literal_values
from meerkat.generated.types import CommsSendParamsPeerLifecycle

# Python 3.10's get_type_hints keeps the typing_extensions Required/NotRequired
# qualifiers that 3.11+ strips.
if sys.version_info >= (3, 11):
    from typing import get_type_hints
else:
    from typing_extensions import get_type_hints


def test_send_lifecycle_kinds_are_the_wire_vocabulary() -> None:
    wire = _literal_values(get_type_hints(CommsSendParamsPeerLifecycle)["lifecycle_kind"])

    assert "mob.dismiss" in wire
    assert _literal_values(MeerkatClient._PeerLifecycleKind) == wire


def test_peer_delivery_outcome_is_a_named_typed_union() -> None:
    from meerkat.generated.types import CommsSendResultPeerMessageSent

    delivery = get_type_hints(CommsSendResultPeerMessageSent)["delivery"]
    durable_arms = [arm for arm in get_args(delivery) if hasattr(arm, "__required_keys__")]
    assert len(durable_arms) == 1, f"durably_resolved is untyped in {delivery}"
    payload = get_type_hints(durable_arms[0])["durably_resolved"]
    outcome = get_type_hints(payload)["outcome"]
    assert _literal_values(outcome) == {"accepted", "deduplicated", "rejected"}
    assert _literal_values(delivery) == {"acked", "volatile_handed_off", "queued"}

    from meerkat import PeerDeliveryOutcome, PeerIngressTerminalOutcomeKind

    assert delivery == PeerDeliveryOutcome
    assert outcome == PeerIngressTerminalOutcomeKind


def test_delivery_validator_domains_are_the_generated_vocabulary() -> None:
    from meerkat import PeerDeliveryOutcome, PeerIngressTerminalOutcomeKind
    from meerkat.client import (
        _PEER_DELIVERY_UNIT_OUTCOMES,
        _PEER_INGRESS_TERMINAL_OUTCOMES,
    )

    assert _PEER_DELIVERY_UNIT_OUTCOMES == _literal_values(PeerDeliveryOutcome)
    assert _PEER_INGRESS_TERMINAL_OUTCOMES == _literal_values(PeerIngressTerminalOutcomeKind)
