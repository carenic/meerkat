"""The generated connector target keeps the typed account-selection alias."""

import sys
import typing

from meerkat.generated import types as generated

# Python 3.10's typing lacks Required, and its get_type_hints keeps the
# typing_extensions qualifiers that 3.11+ strips.
if sys.version_info >= (3, 11):
    from typing import Required, get_type_hints
else:
    from typing_extensions import Required, get_type_hints


def test_account_selection_is_the_named_union_alias() -> None:
    hints = get_type_hints(generated.WireConnectorAuthTarget)
    assert hints["account_selection"] == generated.WireConnectorAccountSelection
    arms = typing.get_args(generated.WireConnectorAccountSelection)
    assert set(arms) == {
        generated.WireConnectorAccountSelectionKnown,
        generated.WireConnectorAccountSelectionDiscover,
    }
    known = get_type_hints(generated.WireConnectorAccountSelectionKnown, include_extras=True)
    assert known["mode"] == Required[typing.Literal["known"]]
    assert known["account"] == Required[str]
    discover = get_type_hints(
        generated.WireConnectorAccountSelectionDiscover, include_extras=True
    )
    assert discover == {"mode": Required[typing.Literal["discover"]]}
