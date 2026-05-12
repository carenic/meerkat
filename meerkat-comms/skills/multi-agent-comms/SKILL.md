---
name: Multi-Agent Comms
description: Setting up keep-alive, peer trust, send vs request/response patterns
requires_capabilities: [comms]
---

# Multi-Agent Communication

Use comms for live collaboration between agents. Comms moves messages and
correlated requests; it is not durable shared work state.

## Operating Rules

- Use keep-alive mode with a `comms_name` when a session should receive peer
  messages after the first turn.
- Use one-way messages for ordinary coordination and request/response only
  when you need a correlated answer.
- Use peer discovery before addressing peers by name.
- Keep durable commitments, claims, dependencies, and terminal outcomes in
  WorkGraph when WorkGraph is available.
- Summarize important peer decisions back into durable artifacts or WorkGraph
  evidence when they matter beyond the live conversation.

## Transport Notes

- UDS is for same-machine low-latency peers.
- TCP is for cross-machine peers.
- In-process transport is for peers in the same runtime.
