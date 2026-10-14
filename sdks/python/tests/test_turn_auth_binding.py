"""Per-turn ``auth_binding`` reaches ``turn/start`` on every turn entry point.

``turn/start`` carries ``auth_binding`` as the canonical Inherit/Set/Clear
tri-state (``StartTurnParams`` in crates/meerkat-rpc/src/handlers/turn.rs):
omit it to keep the session's binding, pass a binding ref or
``{"action": "set", "value": ref}`` to switch, or ``{"action": "clear"}`` to
drop it.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

import pytest

from meerkat import DeferredSession, MeerkatClient, RunResult, Session, Usage
from meerkat.generated.types import WireAuthBindingRef

_RUN_RESULT = {
    "session_id": "s1",
    "text": "ok",
    "turns": 1,
    "tool_calls": 0,
    "usage": {"input_tokens": 1, "output_tokens": 1},
}
_REF = {"realm": "team", "binding": "work-anthropic"}

# (what the caller passes, what turn/start must carry)
_CASES = [
    pytest.param(
        WireAuthBindingRef(realm="team", binding="work-anthropic"), _REF, id="ref"
    ),
    pytest.param(dict(_REF), _REF, id="ref-dict"),
    pytest.param(
        {
            "action": "set",
            "value": WireAuthBindingRef(
                realm="team", binding="work-anthropic", profile="default"
            ),
        },
        {"action": "set", "value": {**_REF, "profile": "default"}},
        id="set",
    ),
    pytest.param({"action": "clear"}, {"action": "clear"}, id="clear"),
]


def _recording_client() -> tuple[MeerkatClient, list[tuple[str, dict[str, Any]]]]:
    client = MeerkatClient()
    calls: list[tuple[str, dict[str, Any]]] = []

    async def fake_request(method: str, params: dict[str, Any]) -> dict[str, Any]:
        calls.append((method, params))
        return dict(_RUN_RESULT)

    client._request = fake_request  # type: ignore[method-assign]
    return client, calls


def _session(client: MeerkatClient) -> Session:
    return Session(client, RunResult(session_id="s1", text="ready", usage=Usage()))


@pytest.mark.asyncio
@pytest.mark.parametrize(("auth_binding", "wire"), _CASES)
async def test_session_turn_sends_auth_binding(auth_binding: Any, wire: Any) -> None:
    client, calls = _recording_client()

    await _session(client).turn("next", auth_binding=auth_binding)

    assert calls == [
        ("turn/start", {"session_id": "s1", "prompt": "next", "auth_binding": wire})
    ]


@pytest.mark.asyncio
@pytest.mark.parametrize(("auth_binding", "wire"), _CASES)
async def test_deferred_start_turn_sends_auth_binding(
    auth_binding: Any, wire: Any
) -> None:
    client, calls = _recording_client()

    await DeferredSession(client, "s1").start_turn("first", auth_binding=auth_binding)

    assert calls == [
        ("turn/start", {"session_id": "s1", "prompt": "first", "auth_binding": wire})
    ]


@pytest.mark.parametrize(("auth_binding", "wire"), _CASES)
def test_session_stream_sends_auth_binding(auth_binding: Any, wire: Any) -> None:
    client = MeerkatClient()
    loop = asyncio.new_event_loop()

    class _Dispatcher:
        def subscribe_events(self, session_id: str) -> asyncio.Queue[Any]:
            return asyncio.Queue()

        def expect_response(self, request_id: int) -> asyncio.Future[Any]:
            return loop.create_future()

    class _Process:
        stdin = object()

    client._dispatcher = _Dispatcher()  # type: ignore[assignment]
    client._process = _Process()  # type: ignore[assignment]
    stream = _session(client).stream("next", auth_binding=auth_binding)
    _, data = stream._pending_send  # type: ignore[misc]
    loop.close()

    request = json.loads(data)
    assert request["method"] == "turn/start"
    assert request["params"]["auth_binding"] == wire


@pytest.mark.asyncio
async def test_turn_without_auth_binding_inherits() -> None:
    client, calls = _recording_client()

    await _session(client).turn("next")

    assert calls == [("turn/start", {"session_id": "s1", "prompt": "next"})]
