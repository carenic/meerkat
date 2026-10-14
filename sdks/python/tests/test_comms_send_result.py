"""``comms/send`` receipts: the SDK accepts exactly the server's shape.

Fixtures are derived from ``CommsSendResult`` in
``artifacts/schemas/wire-types.json``, which ``emit-schemas`` generates from the
Rust ``CommsSendResult`` (crates/meerkat-contracts/src/wire/comms.rs) and its
``PeerDeliveryOutcome`` (crates/meerkat-core/src/comms.rs).
"""

from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any

import pytest

from meerkat import MeerkatClient
from meerkat.errors import MeerkatError

_WIRE_TYPES = (
    Path(__file__).resolve().parents[3] / "artifacts" / "schemas" / "wire-types.json"
)
_SCHEMA = json.loads(_WIRE_TYPES.read_text(encoding="utf-8"))["CommsSendResult"]

# The command that yields each receipt kind.
_COMMANDS: dict[str, dict[str, Any]] = {
    "input_accepted": {"kind": "input", "body": "hello"},
    "peer_message_sent": {"kind": "peer_message", "to": "agent-a", "body": "hello"},
    "peer_lifecycle_sent": {
        "kind": "peer_lifecycle",
        "to": "agent-a",
        "lifecycle_kind": "mob.peer_added",
        "params": {"peer": "agent-b"},
    },
    "peer_request_sent": {
        "kind": "peer_request",
        "to": "agent-a",
        "intent": "checksum_token",
        "params": {"subject": "release-notes"},
    },
    "peer_response_sent": {
        "kind": "peer_response",
        "to": "agent-a",
        "in_reply_to": "00000000-0000-4000-8000-000000000002",
        "status": "completed",
        "result": {
            "request_intent": "checksum_token",
            "request_subject": "release-notes",
            "token": "token-1",
        },
    },
}

# Every `PeerDeliveryOutcome` the server can serialize: externally tagged,
# snake_case, with a `PeerIngressTerminalOutcomeKind` inside
# `durably_resolved` (crates/meerkat-core/src/interaction.rs).
_DELIVERY_FORMS: list[Any] = [
    "acked",
    "volatile_handed_off",
    "queued",
    {"durably_resolved": {"outcome": "accepted"}},
    {"durably_resolved": {"outcome": "deduplicated"}},
    {"durably_resolved": {"outcome": "rejected"}},
]

_INVALID_DELIVERIES: list[Any] = [
    "handed_off",
    "durably_resolved",
    "ACKED",
    "",
    None,
    True,
    1,
    ["acked"],
    {},
    {"acked": None},
    {"durably_resolved": {"outcome": "accepted"}, "acked": None},
    {"durably_resolved": "accepted"},
    {"durably_resolved": None},
    {"durably_resolved": {}},
    {"durably_resolved": {"outcome": "dropped"}},
    {"durably_resolved": {"outcome": None}},
    {"durably_resolved": {"outcome": ["accepted"]}},
    {"durably_resolved": {"outcome": {"accepted": True}}},
    {"durably_resolved": {"outcome": "accepted", "reason": "late"}},
]


def _resolve(node: dict[str, Any]) -> dict[str, Any]:
    ref = node.get("$ref")
    if ref is None:
        return node
    return _SCHEMA["$defs"][ref.removeprefix("#/$defs/")]


def _instances(node: dict[str, Any]) -> list[Any]:
    """Every value the schema node admits, one representative per string."""
    node = _resolve(node)
    if "oneOf" in node:
        return [value for arm in node["oneOf"] for value in _instances(arm)]
    if "const" in node:
        return [node["const"]]
    if "enum" in node:
        return list(node["enum"])
    if node["type"] == "string":
        return ["00000000-0000-4000-8000-000000000001"]
    if node["type"] == "boolean":
        return [False, True]
    assert node["type"] == "object", node
    values: list[dict[str, Any]] = [{}]
    for name, prop in node["properties"].items():
        values = [{**value, name: item} for value in values for item in _instances(prop)]
    return values


_RECEIPTS = _instances(_SCHEMA)
_ARMS = {arm["properties"]["kind"]["const"]: arm for arm in _SCHEMA["oneOf"]}
_PEER_KINDS = sorted(kind for kind in _ARMS if kind != "input_accepted")


def _receipt_id(receipt: dict[str, Any]) -> str:
    return "-".join(
        json.dumps(receipt[key], separators=(",", ":")).strip('"')
        for key in ("kind", "delivery", "stream_reserved")
        if key in receipt
    )


def _client_returning(response: Any) -> MeerkatClient:
    client = MeerkatClient()

    async def fake_request(method: str, params: dict[str, Any]) -> Any:
        assert method == "comms/send"
        return copy.deepcopy(response)

    client._request = fake_request  # type: ignore[method-assign]
    return client


async def _assert_rejected(response: Any, kind: str = "peer_message_sent") -> None:
    client = _client_returning(response)
    with pytest.raises(MeerkatError) as excinfo:
        await client.send("s1", **_COMMANDS[kind])
    assert excinfo.value.code == "INVALID_RESPONSE"


def test_schema_fixtures_cover_every_receipt_kind_and_delivery_form() -> None:
    assert sorted({receipt["kind"] for receipt in _RECEIPTS}) == sorted(_COMMANDS)
    peer_receipts = [r for r in _RECEIPTS if r["kind"] != "input_accepted"]
    assert all("delivery" in receipt for receipt in peer_receipts)
    for kind in _PEER_KINDS:
        assert "delivery" in _ARMS[kind]["required"]
    assert {
        json.dumps(receipt["delivery"], sort_keys=True) for receipt in peer_receipts
    } == {json.dumps(form, sort_keys=True) for form in _DELIVERY_FORMS}


@pytest.mark.asyncio
@pytest.mark.parametrize("receipt", _RECEIPTS, ids=_receipt_id)
async def test_send_accepts_every_server_receipt(receipt: dict[str, Any]) -> None:
    client = _client_returning(receipt)

    result = await client.send("s1", **_COMMANDS[receipt["kind"]])

    assert result == receipt


@pytest.mark.asyncio
@pytest.mark.parametrize("kind", sorted(_ARMS))
async def test_send_rejects_unknown_receipt_field(kind: str) -> None:
    receipt = _instances(_ARMS[kind])[0]
    await _assert_rejected({**receipt, "extra_behavior": True}, kind)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("kind", "field"),
    [(kind, field) for kind in sorted(_ARMS) for field in _ARMS[kind]["required"]],
)
async def test_send_rejects_receipt_missing_required_field(kind: str, field: str) -> None:
    receipt = _instances(_ARMS[kind])[0]
    del receipt[field]
    await _assert_rejected(receipt, kind)


@pytest.mark.asyncio
@pytest.mark.parametrize("kind", _PEER_KINDS)
@pytest.mark.parametrize(
    "delivery",
    _INVALID_DELIVERIES,
    ids=lambda value: json.dumps(value, separators=(",", ":")),
)
async def test_send_rejects_invalid_delivery(kind: str, delivery: Any) -> None:
    receipt = _instances(_ARMS[kind])[0]
    await _assert_rejected({**receipt, "delivery": delivery}, kind)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "response",
    [
        None,
        [],
        "peer_message_sent",
        {},
        {"kind": "peer_message", "envelope_id": "env-1", "delivery": "acked"},
        {"kind": "legacy_sent", "envelope_id": "env-1", "delivery": "acked"},
    ],
    ids=lambda value: json.dumps(value, separators=(",", ":")),
)
async def test_send_rejects_non_receipt_results(response: Any) -> None:
    await _assert_rejected(response)
