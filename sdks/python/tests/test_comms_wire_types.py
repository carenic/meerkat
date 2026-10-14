"""Hand-written comms/send typing agrees with the generated wire contracts."""

from __future__ import annotations

import sys

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
