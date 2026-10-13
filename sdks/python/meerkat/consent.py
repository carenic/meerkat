"""R3 human consent notices: ``approval/consent_notice``.

A connection that serves R3 human consent (a stock governed pipe) delivers one
notice per recorded consent transition of an approval its approver decides. A
``requested`` notice carries the approval's presentation: what its approver is
asked to decide, as the approval owner recorded it. A notice grants nothing:
an ``approved`` one executes nothing by itself.
"""

from __future__ import annotations

from typing import Any, cast

from .generated.types import ConsentNoticeKind, ConsentNoticeParams

CONSENT_NOTICE_NOTIFICATION = "approval/consent_notice"

_CONSENT_NOTICE_KINDS = frozenset({"requested", "approved", "denied", "used", "withdrawn"})
_APPROVAL_RISKS = frozenset({"low", "medium", "high", "critical"})
_CONSENT_ASSURANCES = frozenset({"device_trusted", "person_authenticated"})


def _known(value: Any, known: frozenset[str]) -> bool:
    return isinstance(value, str) and value in known


def _parse_presentation(value: Any) -> dict[str, Any] | None:
    """Narrow one notice's presentation.

    Returns ``None`` for a malformed one or one naming a risk or an assurance
    this SDK build does not know.
    """
    if not isinstance(value, dict):
        return None
    action = value.get("action")
    if (
        not isinstance(action, dict)
        or not isinstance(action.get("kind"), str)
        or not isinstance(action.get("summary"), str)
        or not _known(value.get("risk"), _APPROVAL_RISKS)
        or not _known(value.get("accepted_assurance"), _CONSENT_ASSURANCES)
        or not isinstance(value.get("expires_at"), str)
        or not isinstance(value.get("account"), (str, type(None)))
        or not isinstance(value.get("mandate"), (str, type(None)))
    ):
        return None
    presentation = {
        "action": action,
        "risk": value["risk"],
        "accepted_assurance": value["accepted_assurance"],
        "expires_at": value["expires_at"],
    }
    for statement in ("account", "mandate"):
        if isinstance(value.get(statement), str):
            presentation[statement] = value[statement]
    return presentation


def parse_consent_notice(params: Any) -> ConsentNoticeParams | None:
    """Narrow one ``approval/consent_notice`` payload to its typed params.

    Returns ``None`` for a malformed payload, a transition kind this SDK build
    does not know, or a presentation it cannot read, so a newer server's
    notices are ignored, never misread.
    """
    if not isinstance(params, dict):
        return None
    approval_id = params.get("approval_id")
    tool_call_id = params.get("tool_call_id")
    kind = params.get("kind")
    if (
        not isinstance(approval_id, str)
        or not isinstance(tool_call_id, str)
        or not _known(kind, _CONSENT_NOTICE_KINDS)
    ):
        return None
    raw = params.get("presentation")
    presentation = None if raw is None else _parse_presentation(raw)
    if raw is not None and presentation is None:
        return None
    return ConsentNoticeParams(
        approval_id=approval_id,
        tool_call_id=tool_call_id,
        kind=cast(ConsentNoticeKind, kind),
        presentation=presentation,
    )
