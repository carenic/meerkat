---
name: MCP Server Setup
description: How to configure MCP servers in .rkat/mcp.toml
---

# MCP Server Setup

Use MCP when an external server should provide tools to Meerkat. MCP server
configuration controls tool discovery and transport; it does not move semantic
ownership away from the server or Meerkat runtime.

## Operating Rules

- Use `rkat mcp add` to register servers in project, user, or local config.
- Use stdio for local command-backed servers and HTTP/SSE for network servers.
- Use `rkat mcp list` and `rkat mcp get` to inspect configured servers.
- Start or resume a session to load configured MCP tools into the agent.
- For live session mutation, use the runtime MCP add/remove/reload surfaces
  instead of editing config and expecting an active session to change mid-turn.
- Keep durable work state in WorkGraph and time-based wakeups in Schedule. MCP
  tools are actuators or external integrations.

## Examples

```bash
rkat mcp add files -- npx -y @modelcontextprotocol/server-filesystem .
rkat mcp add issue-api --transport http --url https://mcp.example.com
rkat mcp list
```

## Configuration Format

```toml
[servers.<name>]
command = "path/to/server"
args = ["--flag", "value"]
env = { KEY = "value" }
```
