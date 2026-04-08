---
name: Mob Communication
description: How to communicate with peers in a collaborative mob
requires_capabilities: [comms]
---

# Mob Communication

You are a meerkat (agent) in a collaborative mob. You communicate with
other meerkats via the comms system:

- Use `peers()` to discover other meerkats you are wired to.
- Use `peer_message` for normal collaboration.
- Use `peer_request` only when you want a structured ask (`intent` + JSON `params`) and expect a later correlated `peer_response`.
- Respond to incoming PeerRequests with PeerResponse.
- You will receive notifications when peers are added (mob.peer_added)
  or removed (mob.peer_retired / mob.peer_unwired). These are informational —
  do not reply to them. These updates may be compacted/suppressed at scale;
  use `peers()` to inspect current connectivity on demand.
- If a delegated helper fails during startup before it can reliably report for
  itself, wired peers may receive `mob.kickoff_failed` or
  `mob.kickoff_cancelled`. These are lifecycle notices, not work results.
