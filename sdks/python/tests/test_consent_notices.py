"""``approval/consent_notice`` reaches on_consent_notice callbacks, typed."""

import asyncio
import json

import pytest

from meerkat import CONSENT_NOTICE_NOTIFICATION, MeerkatClient, parse_consent_notice
from meerkat.generated.types import ConsentNoticeParams
from meerkat.streaming import _StdoutDispatcher

APPROVAL = "01936f8a-7b2c-7000-8000-0000000000c1"


def _reader(lines: list[dict]) -> asyncio.StreamReader:
    reader = asyncio.StreamReader()
    for line in lines:
        reader.feed_data((json.dumps(line) + "\n").encode())
    reader.feed_eof()
    return reader


def _notice(kind: str) -> dict:
    return {"approval_id": APPROVAL, "tool_call_id": "toolu_01", "kind": kind}


def test_parse_consent_notice_types_every_known_kind():
    assert CONSENT_NOTICE_NOTIFICATION == "approval/consent_notice"
    for kind in ("requested", "approved", "denied", "used", "withdrawn"):
        assert parse_consent_notice(_notice(kind)) == ConsentNoticeParams(
            approval_id=APPROVAL, tool_call_id="toolu_01", kind=kind
        )


_PRESENTATION = {
    "action": {"kind": "tool_call", "summary": "read", "body": {"folder": "inbox"}},
    "risk": "high",
    "account": "alice at https://issuer.example",
    "mandate": "the work's mandate",
    "accepted_assurance": "device_trusted",
    "expires_at": "2030-01-02T03:04:05Z",
}


def test_a_requested_notice_carries_its_presentation():
    notice = {**_notice("requested"), "presentation": _PRESENTATION}
    assert parse_consent_notice(notice) == ConsentNoticeParams(
        approval_id=APPROVAL,
        tool_call_id="toolu_01",
        kind="requested",
        presentation=_PRESENTATION,
    )
    # An owner statement that is absent stays absent.
    unstated = {k: v for k, v in _PRESENTATION.items() if k not in ("account", "mandate")}
    parsed = parse_consent_notice(
        {**_notice("requested"), "presentation": {**unstated, "account": None}}
    )
    assert parsed is not None and parsed.presentation == unstated
    parsed = parse_consent_notice({**_notice("requested"), "presentation": None})
    assert parsed is not None and parsed.presentation is None


def test_a_presentation_this_build_cannot_read_makes_the_notice_ignored():
    for presentation in (
        "high",
        [],
        {**_PRESENTATION, "action": "read"},
        {**_PRESENTATION, "action": {"kind": "tool_call"}},
        {**_PRESENTATION, "risk": "extreme"},
        {**_PRESENTATION, "risk": {}},
        {**_PRESENTATION, "accepted_assurance": "hardware_key"},
        {**_PRESENTATION, "expires_at": 7},
        {**_PRESENTATION, "account": 7},
    ):
        assert parse_consent_notice({**_notice("requested"), "presentation": presentation}) is None


def test_a_malformed_or_unknown_consent_notice_is_ignored():
    for params in (
        None,
        [],
        "requested",
        {"tool_call_id": "toolu_01", "kind": "requested"},
        {"approval_id": APPROVAL, "tool_call_id": 7, "kind": "requested"},
        {"approval_id": APPROVAL, "tool_call_id": "toolu_01", "kind": "granted"},
        {"approval_id": APPROVAL, "tool_call_id": "toolu_01", "kind": {}},
    ):
        assert parse_consent_notice(params) is None


@pytest.mark.asyncio
async def test_dispatcher_routes_consent_notices_to_the_sink_not_a_session_queue():
    seen: list[dict] = []
    reader = _reader(
        [
            {"jsonrpc": "2.0", "method": CONSENT_NOTICE_NOTIFICATION, "params": _notice("requested")},
            # A sentinel response orders the assertions after the notice.
            {"jsonrpc": "2.0", "id": 7, "result": {}},
        ]
    )
    dispatcher = _StdoutDispatcher(reader)
    dispatcher.set_consent_notice_sink(seen.append)
    dispatcher.start()
    await asyncio.wait_for(dispatcher.expect_response(7), timeout=1.0)
    assert seen == [_notice("requested")]
    assert dispatcher._event_queues == {}
    assert dispatcher._unmatched_buffer == {}
    await dispatcher.stop()


def test_client_callbacks_receive_typed_notices_and_unsubscribe():
    client = MeerkatClient()
    seen: list[ConsentNoticeParams] = []

    def faulty(_notice: ConsentNoticeParams) -> None:
        raise RuntimeError("callback fault")

    client.on_consent_notice(faulty)
    unsubscribe = client.on_consent_notice(seen.append)
    client._dispatch_consent_notice(_notice("requested"))
    client._dispatch_consent_notice(_notice("granted"))
    assert seen == [
        ConsentNoticeParams(approval_id=APPROVAL, tool_call_id="toolu_01", kind="requested")
    ]
    unsubscribe()
    client._dispatch_consent_notice(_notice("approved"))
    assert len(seen) == 1
