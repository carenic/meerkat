Covers: Skills in Meerkat 0.9. The SKILL.md format, frontmatter parsing rules
(0.9.4) and extension values, skill identity (`source_uuid` + `skill_name`), skill
sources (embedded built-ins, `[[skills.repositories]]` git/http/filesystem/stdio,
project `.rkat/skills`), the `[skills]` config, `rkat skill add|remove|get|list|inspect`,
`rkat run --skill`, `preload_skills` and `skill_refs` (RPC, REST, SDKs), mob and
mobpack skills, introspection (`skills/list`, `GET /skills`) and skill errors.

# Skills (0.9)

Names are exact: anything not listed does not exist. Tools are in
`mcp_tools_skills_tools.md`; MCP servers are in `mcp_tools_skills_mcp.md`.

## Quick answers

- Add your own skill to a project: create `<project>/.rkat/skills/<name>/SKILL.md`.
  Or register a directory elsewhere: `rkat skill add ./path/to/skills`.
- See what is available: `rkat skill list --json` (keys include each skill's
  `source_uuid`).
- Read one skill: `rkat skill inspect task-workflow --source-uuid
  00000000-0000-4b11-8111-000000000001`.
- Use a skill in a session: `preload_skills` at create (whole session) or
  `skill_refs` on a turn (that turn only). `rkat run --skill <name>` works for
  embedded built-ins only.

## Skill format and identity

A skill is a directory holding `SKILL.md` plus optional resource files:

```markdown
---
name: release-notes
description: Draft release notes from merged changes.
---
Steps the agent should follow ...
```

- Frontmatter: `name` (must equal the directory name) and `description` are
  required; `requires_capabilities`, `metadata` (a map of text) and `version`
  are optional. Any other key must be a namespaced extension key
  (`vendor.key`); a mapping or sequence value needs a `vendor.version` key
  beside it.
- Since 0.9.4 the frontmatter is parsed as one YAML 1.2 document by a
  pure-Rust parser (no Meerkat build includes `unsafe-libyaml` any more).
  `yes`, `on` and `off` stay strings, `<<` is an ordinary key, and a second
  document is refused. Also refused, where 0.9.3 accepted them: a key given
  twice at any level (0.9.3 kept the last), a null (`~` or an empty value) as
  `description` or a `metadata` value, an unrecognized tag such as `!custom`,
  nesting deeper than 64 levels, 100 or more aliases that outnumber their
  anchors more than ten to one, and a `...` document end marker line. A block
  scalar (`|`, `|+`, `>`) that ends the frontmatter keeps its final line break.
- Extension values (`SkillDocument.extensions`) are text: a string as
  written, any other value as YAML rendered from its resolved value, not the
  source text (`1.10` is `1.1`, `1_000` is `1000`, `tRUE` is `true`, `6.02e23`
  is `6.02e+23`). Quote a value with leading zeros, such as a file mode
  (`'0755'`): unquoted, `0755` becomes `755.0`.
- Names are lowercase slugs: `a-z`, `0-9` and `-`, not starting or ending with
  `-`, no `--`.
- Identity is a `SkillKey {source_uuid, skill_name}`: the same name in two
  sources is two skills (shadowing is by full key).

## Sources

Sources, in order:

1. Embedded built-ins, source `00000000-0000-4b11-8111-000000000001`:
   `session-management`, `task-workflow`, `shell-patterns`,
   `builtin-utilities-workflow`, `skill-discovery-workflow`,
   `mcp-server-setup`, `memory-retrieval`, `schedule-workflow`,
   `workgraph-workflow`, `multi-agent-comms`, `mob-communication`,
   `hook-authoring`, `meerkat-platform`, `meerkat-cli-reference` (each when its
   component is compiled in).
2. `[[skills.repositories]]` entries of the composed realm config, in order.
3. `<context-root>/.rkat/skills/` when present (searched recursively for
   directories holding `SKILL.md`), source `00000000-0000-4b11-8111-000000000002`.
   There is no user-wide skills directory.

- A repository that fails to resolve degrades the runtime to the embedded
  skills, with a warning.
- Skills whose `requires_capabilities` are unavailable are hidden, and
  resolving one fails.
- Browser (WASM) runtimes use embedded skills only.

## Configuration

```toml
[skills]
enabled = true              # default
max_injection_bytes = 32768 # default
inventory_threshold = 12    # default

[[skills.repositories]]
name = "team"
source_uuid = "8f9c2d4e-1b3a-4c5d-9e8f-0a1b2c3d4e5f"
type = "git"                # or "filesystem" (path), "http" (url), "stdio" (command)
url = "https://git.example.com/team/skills.git"
git_ref = "main"            # ref_type = "branch" | "tag" | "commit"
```

- Every repository needs `name` and `source_uuid`.
- `filesystem`: `path`, relative to the context root.
- `git`: also takes `skills_root`, `auth = { scheme = "token", token = "..." }`
  or `{ scheme = "ssh-key", path = "..." }`, `refresh_seconds` (300) and `depth`
  (1). Clones are cached under `.rkat/skill-cache/git` in the realm's runtime
  root.
- `http`: `url`, `auth = { scheme = "bearer", token = "..." }` or
  `{ scheme = "header", name = "...", value = "..." }`, `refresh_seconds` and
  `timeout_seconds` (15). It needs the `skills-http` build feature and follows
  same-origin redirects only.
- `stdio`: `command`, `args`, `cwd`, `env`, `timeout_seconds` (15).

## CLI

```bash
rkat skill add <PATH> [--name <NAME>]       # filesystem source in this realm's own config
rkat skill remove <NAME_OR_SOURCE_UUID_OR_PATH>
rkat skill get <NAME_OR_SOURCE_UUID_OR_PATH> [--json]
rkat skill list [--json]
rkat skill inspect <SKILL_NAME> --source-uuid <SOURCE_UUID> [--json]
```

- `add` takes a skill directory, a `SKILL.md` file or a repository root. It
  writes only the selected realm's `skills.repositories` (with a source UUID
  derived from the path), sets `skills.enabled = true`, and refuses a duplicate
  (`Skill source '<name>' is already configured`).
- `remove` of a source configured in a parent realm is refused; remove it in
  the realm that configures it.
- `rkat run --skill <NAME>` (repeatable) preloads only an embedded built-in by
  name. A path or a source added with `rkat skill add` is not preloaded that
  way: use `preload_skills` with its source UUID.
- There is no `rkat skills` (plural).

## How skills reach the model

- Inventory: when builtins are on, the system prompt lists the available
  skills (flat up to `inventory_threshold`, summarized above it).
- `preload_skills: [{"source_uuid": "...", "skill_name": "..."}]` on session
  create (RPC `session/create`, REST `POST /sessions`, SDKs) puts the full
  skills in the system prompt.
- `skill_refs: [{"kind": "structured", "source_uuid": "...", "skill_name":
  "..."}]` on RPC `session/create` and `turn/start`, or REST create and
  continue, injects the skills for that turn, before the user message. RPC
  refuses the retired string field `skill_references`.
- SDKs: Python `Session.invoke_skill(SkillKey(source_uuid=..., skill_name=...),
  prompt)`; TypeScript `session.invokeSkill({sourceUuid, skillName}, prompt)`;
  `skill_refs` / `skillRefs` on turns.
- The agent skill tools (`browse_skills`, `load_skill`,
  `skill_list_resources`, `skill_read_resource`, `skill_invoke_function`) are
  off by default; only an explicit host tool policy enables them.
- Mob profiles: `skills = ["triage"]` names `[skills.triage]` entries of the
  mob definition (`source = "inline"` with `content`, or `source = "path"`
  with `path`); the skill text becomes part of the member's prompt. A mobpack
  inlines path skills from its `skills/` directory; browser runtimes refuse
  path skills.

## Introspection and errors

- CLI `rkat skill list|inspect`; RPC `skills/list` (`skills/inspect` is not
  served: method not found); REST `GET /skills`; `rkat-mcp` tool
  `meerkat_skills {action: "list" | "inspect", skill_key?, source?}`; Python
  `list_skills()`; TypeScript `listSkills()`; Rust
  `SkillRuntime::list_all_with_provenance` and `load_from_source`.
- RPC errors: `SKILL_NOT_FOUND` (-32021), `SKILL_RESOLUTION_FAILED` (-32022);
  `skills/list` answers "skills not enabled" when the host has no skill runtime.
- A turn that names skills on an agent built without skills fails before any
  model call (`no_skill_engine`).
