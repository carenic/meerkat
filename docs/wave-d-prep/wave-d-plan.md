# Wave-d plan — "Fully 100% functional system"

## Scope

Wave-d closes the gap between "wave-c spine landed" and **a fully functional Meerkat 0.6.0: all tests green including e2e-smoke, CI green, SDK types synced, `make ci` zero locally + GitHub gate green on main**. After wave-d the only remaining work is updating docs and releasing it. **There is no wave-e** — every architectural or compile debt either closes in wave-d or is explicitly filed as a GitHub issue for a future release (0.7.x / 1.0.0), never as "defer to wave-e".

## What wave-d is

- Structural dogma closure — the 8 audit findings from the post-wave-c machine survey
- Compile-debt closure — meerkat-mob cascade tail, Track-B producer wiring, any per-crate residue
- Fmt + clippy sweep — end of `--no-verify` tolerance
- Test lanes — unit, int, e2e-fast / e2e-system / **e2e-smoke** / e2e-live / e2e-models all green
- SDK sync — generated types regenerated from wave-c+d wire changes, parity-verified, SDK test suites green (Python / TypeScript / Web)
- `make ci` zero locally + GitHub gate green on post-merge main
- Squash-merge `dogma/wave-a-demolition` → `main`

## What wave-d is NOT (deferred to the post-wave-d docs + release phase)

- Version bump (stays on 0.6.0; release tag is a separate mechanical step)
- Documentation regeneration from typed catalog (auto-generated API docs + prose)
- User-facing documentation audit (docs/guides/*.mdx)
- Tagged release (`git tag v0.6.0` + `release.yml` workflow trigger)
- Registry publishes (crates.io, PyPI, npm, GitHub release binaries + checksums)
- CHANGELOG curation

These are the final-release step and come after wave-d lands a green tree. **They are NOT a "wave-e" — they are the release execution of 0.6.0.**

## Success criterion

Wave-d is done when the tree is in a state where "tag the release + update docs" would succeed without code changes. The tree builds, tests pass, CI is green, nothing deferred to a future architectural wave.

---

## Section 1 — Integrated task list

Tasks numbered by team task-ID after their d-phase label. Current in-flight / pending (wave-c tail) still sit alongside — wave-d tasks are #38-#45 already created + the new compile-debt / test-lane / CI tasks added below.

### d.0 — Structural dogma closure (8 audit findings)

All from the post-wave-c machine sanity audit. Each is fix-only; no document-by-design rationales.

- **#38 · D-a AuthMachine Release emits EmitLifecycleEvent** — `auth_machine.rs:159-162` missing terminal effect; observer can't determine release. Add emit; `grep emit auth_machine.rs | wc -l` → 12.
- **#39 · D-b MobMachine.topology_epoch increment** — field declared + initialized but no write site found in audit; either increment is missing (add to WireMembers/UnwireMembers/Bind/Rotate/Release transitions) or it's present somewhere off-audit (verify). Catching: `grep "topology_epoch.*=" mob_machine.rs` shows ≥1 write.
- **#40 · D-c AuthMachine composition** — zero references in compositions.rs, mandatory per user directive. New composition spec linking AuthMachine to MeerkatMachine session lifecycle.
- **#41 · D-d SupervisorTrustBridge ack epoch threading** — effect carries epoch, ack doesn't. Add `epoch: u64` to SupervisorTrustEdgePublished/Failed ack inputs + DSL guard on match.
- **#42 · D-e PeerEndpoint twin symmetric From impl + revive tripwire** — runtime side has `From<&TrustedPeerDescriptor>`, schema side doesn't. Add to schema twin OR lift to shared module. Verify `peer_endpoint_structural_equivalence` tripwire still live.
- **#43 · D-f Schedule SupersedePendingOccurrences reciprocal ack** — one-way route; add `OccurrencesSuperseded` effect + `ConfirmOccurrencesSuperseded` input on Schedule side + matching composition route.
- **#44 · D-g Schedule Delete idempotency from Deleted phase** — add DeleteDeleted transition or refactor to single idempotent Delete input with inclusive guards.
- **#45 · D-h OccurrenceLifecycleMachine Paused-planning race** — remove RecordPlanningWindowPaused transition entirely OR guard on schedule-paused state via composition route.

### d.1 — Compile debt closure

- **#46 · D-mob meerkat-mob lib 86-error cascade** — `CanonicalMemberStatus` / `CanonicalSessionObservation` / `CanonicalMemberSnapshotMaterial` / `MobMemberLifecycleInput` undeclared in `meerkat-mob/src/runtime/actor.rs`; `TrustedPeerDescriptor::new` → `test_only_unsigned` rename missed at 2 production sites; `StartTurnRequest` field tail (execution_kind/additional_instructions — collapsed in C-1, 2+ consumer sites remain); `MemberDeliveryReceipt.fence_token` missing. Some are codegen-regenerable (run `make machine-codegen` first); remainder are mechanical consumer updates. Investigate with `cargo check -p meerkat-mob --lib` then close each class.
- **#47 · D-track-b Track-B peer-projection producer wiring** — from `docs/wave-d-prep/track-b-producer-wiring.md` (c-37's finding): three seams (stager helpers for AddDirectPeerEndpoint/RemoveDirectPeerEndpoint/ApplyMobPeerOverlay; WireMember/UnwireMember bridge re-route through stager; CommsTrustReconciler lifetime + observer-consumer of CommsTrustReconcileRequested effect). Closes the effect emitter→consumer gap properly (not documentation).
- **#48 · D-workspace workspace-wide cargo check zero-exit** — after D-mob + D-track-b land, `./scripts/repo-cargo check --workspace --all-targets` must exit zero. Any residual per-crate errors (outside #46/#47 scope) close as part of this task.

### d.2 — Tree hygiene

- **#49 · D-fmt workspace-wide cargo fmt clean** — drift accumulated across wave-c from agents using `--no-verify` to bypass fmt hook. Single sweep: `cargo fmt --all` locally; commit the result. Per-file verification: `cargo fmt --all -- --check` zero-exit post-commit.
- **#50 · D-clippy workspace-wide clippy clean** — `cargo clippy --all-targets -- -D warnings` zero-exit. Triage + fix any clippy findings. Do NOT add `#[allow(clippy::*)]` as a shortcut — fix the code or document the allow in a focused-exception manner.

### d.3 — Test lanes

- **#51 · D-tests-fast unit + int + e2e-fast green** — `cargo unit`, `cargo int`, `cargo e2e-fast` all zero-fail. Fix any test breakage surfaced.
- **#52 · D-tests-system e2e-system green** — `cargo e2e-system` zero-fail.
- **#53 · D-tests-smoke e2e-smoke green** — `cargo e2e-smoke` (live-provider lane) zero-fail. Mandatory per user direction. This lane hits real APIs; if hits are genuinely external (rate-limit, API drift), the fix is to retry with an explicit harness, not mask with `#[ignore]`. No deferrals.
- **#54 · D-tests-live e2e-live green** — `cargo e2e-live` (targeted live-provider lane). If provider drift blocks a specific test, fix the drift (update model ID, update catalog); don't `#[ignore]`.
- **#55 · D-tests-models e2e-models green** — `cargo e2e-models` (per-model catalog validation). Same rule — fix catalog drift.

### d.4 — SDK sync

- **#56 · D-sdk-regen schemas + SDK generated types regen** — `make regen-schemas` produces zero-diff after commit (fresh regen from post-d.3 contracts). `verify-schema-freshness.sh` zero-exit on all 8 schemas. This is a second regen pass on top of wave-c's C-REGEN — wave-d's d.0 structural changes (AuthMachine composition, SupervisorTrustBridge epoch ack, Schedule supersede reciprocal, etc.) introduced new wire shapes that need to flow into the Python / TypeScript / Web generated types.
- **#57 · D-sdk-parity `make verify-version-parity` zero-exit** — all 6 version anchors agree (workspace Cargo.toml, ContractVersion, Python pyproject, TS package.json, Web package.json, schemas/version.json). Stays on 0.6.0; parity verification, not bump.
- **#58 · D-sdk-python Python SDK test suite green** — `cd sdks/python && pytest` (or equivalent) zero-fail. Build artifact validates via `twine check` dry-run.
- **#59 · D-sdk-typescript TypeScript SDK build + test green** — `cd sdks/typescript && npm test && npm run build` zero-exit. `npm publish --dry-run` validates.
- **#60 · D-sdk-web Web SDK build + typecheck green** — `cd sdks/web && npm run build && npm run typecheck` zero-exit.

### d.5 — CI gate + merge

- **#61 · D-ci-green `make ci` zero locally** — the full CI chain (fmt-check, legacy-surface-gate, verify-version-parity, lint, lint-feature-matrix, test-all, test-minimal, test-feature-matrix, test-surface-modularity, rmat-audit, audit) passes locally without `--no-verify`.
- **#62 · D-merge squash-merge into main + GitHub gate green** — `git checkout main && git merge --squash dogma/wave-a-demolition`, single commit message summarizing waves a/b/c/d outcomes. Push. GitHub `gate` workflow green on the merge commit.

Task count: **8 (d.0) + 3 (d.1) + 2 (d.2) + 5 (d.3) + 5 (d.4) + 2 (d.5) = 25 tasks**.

---

## Section 2 — Dependency graph

Serial spine:
```
d.0 (structural prerequisites for correctness invariants)
  ↓
d.1 (compile debt — needs d.0 done; audit findings touch same files)
  ↓
d.2 (tree hygiene — needs d.1 done; fmt/clippy only matter when compile green)
  ↓
d.3 (test lanes — needs d.2 done; tests need compile + fmt + clippy green)
  ↓
d.4 (SDK sync — needs d.3 done; contracts frozen after tests prove wire stability)
  ↓
d.5 (CI + merge — needs d.4 done)
```

Parallelism:
- **Within d.0**: 8 tasks largely independent. Can spawn 4-8 agents in parallel.
- **Within d.1**: #46 + #47 parallel. #48 serializes on both.
- **Within d.2**: #49 + #50 parallel.
- **Within d.3**: Test lanes can fail-independently; assign to different agents for parallel debugging.
- **Within d.4**: #58 (Python) + #59 (TypeScript) + #60 (Web) parallel after #56 (regen) + #57 (parity) land serially.
- **d.5 is serial** (single agent closing the final gate).

---

## Section 3 — Parallelism / worktree strategy

Same pattern as wave-c: pre-create per-task worktree, agent works on dedicated branch, coordinator merges back. `dogma/wave-d-<task-slug>` branch naming.

**Fan-out windows**:
- **d.0 fan-out**: 8 parallel agents on structural dogma tasks. Light-to-moderate per-task (~50-300 LOC each). Good test of the agent-operating-envelope doc's wave-d uptake.
- **d.1 fan-out**: #46 + #47 parallel. #47 is larger (three architectural seams); #46 is mechanical cascade.
- **d.2 tasks run alone**: fmt must land before clippy (fmt changes don't trigger clippy; clippy changes don't trigger re-fmt if agent is disciplined). Serial.
- **d.3 in parallel**: fast + system + smoke + models on separate agents if each lane fails for independent reasons.

**Agent pool**: at wave-d kickoff the live agents are c-37-trust-reconciler-wire (light context, Track-B expert = ideal #47 owner), c-1-core-retype (heavy context), c-10-rest-retype (moderate), c-36-codegen-fix (moderate, just finished codegen). Fresh agents for d.0 likely — rotation discipline favors fresh hands for new task-classes.

---

## Section 4 — Phases within wave-d

- **d.0 — Structural dogma closure.** 8 audit findings land as distinct commits. Exit gate: all 8 tasks completed; `xtask::machines::check_dsl_parity` passes; `rmat-audit --strict` zero findings; AuthMachine appears in ≥1 composition spec.
- **d.1 — Compile debt closure.** meerkat-mob lib + Track-B producer wiring + residual crate-level breakage. Exit gate: `cargo check --workspace --all-targets` zero-exit.
- **d.2 — Tree hygiene.** Fmt + clippy clean workspace-wide, no more `--no-verify` tolerance. Exit gate: `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` both zero-exit.
- **d.3 — Test lanes.** Every lane green: `cargo unit`, `cargo int`, `cargo e2e-fast`, `cargo e2e-system`, **`cargo e2e-smoke`**, `cargo e2e-live`, `cargo e2e-models`. Exit gate: zero failures across the lane set.
- **d.4 — SDK sync.** Generated types synced with post-d.3 contracts; Python / TypeScript / Web SDKs build + test green; parity verified. Exit gate: `make verify-version-parity` + `make verify-schema-freshness` both zero-exit; each SDK's build + test zero-fail.
- **d.5 — CI gate + merge.** `make ci` + squash-merge onto main + GitHub gate green. Exit gate: `main` is the new authoritative branch with all wave-a/b/c/d work, ready for docs + release phase.

---

## Section 5 — Risk register

1. **meerkat-mob cascade expands during #46 execution.** The 86-error cascade may resolve partially to reveal deeper architectural gaps (e.g., missing generated types from DSL regen). Mitigation: scope-cap at 500 LOC / 20 files; if blown, stop and report. Per wave-c experience, codegen regen often resolves ~60% of seemingly-separate errors.
2. **Track-B producer wiring (#47) crosses into new architectural integration.** Three seams — if any one requires new DSL state fields, widen scope per architectural-prerequisite rule; don't split into follow-ups.
3. **Live-provider test lanes flaky.** `e2e-smoke` and `e2e-live` hit external APIs. Mitigation: the fix is to harden the test (retry harness, better provider-drift handling in the catalog, updated model IDs), not `#[ignore]`. No deferrals — user direction is these must pass in wave-d.
4. **Clippy findings hidden behind `--no-verify` for all of wave-c** may be substantial. Mitigation: triage into (a) real code smells to fix, (b) legitimate-per-context `#[allow]` with explanatory comment. Don't mass-allow.
5. **Fmt sweep introduces cross-worktree conflicts.** `cargo fmt --all` touches many files; if other agents have mid-work changes on the same files, merge conflicts. Mitigation: fmt sweep runs last in d.2 — after all d.1 work is merged.
6. **Squash-merge loses forensic detail.** User accepted squash per "wholesale rewrite of main" framing. The `dogma/wave-a-demolition` branch can be kept as a tag (`archive/wave-a-demolition`) for forensic access after the squash.

---

## Section 6 — Completion criteria

Wave-d is done when ALL hold:

- All 25 tasks (#38-#62) marked completed.
- `./scripts/repo-cargo check --workspace --all-targets` exits zero.
- `./scripts/repo-cargo nextest run --workspace` passes with zero failures.
- `./scripts/repo-cargo clippy --workspace -- -D warnings` passes zero-exit.
- `./scripts/repo-cargo fmt --all -- --check` exits zero.
- Every test lane green: `cargo unit`, `cargo int`, `cargo e2e-fast`, `cargo e2e-system`, `cargo e2e-smoke`, `cargo e2e-live`, `cargo e2e-models`.
- SDK parity: `make verify-version-parity` + `make verify-schema-freshness` both zero-exit.
- SDK builds + tests: Python `pytest` zero-fail, TypeScript `npm test && npm run build` zero-exit, Web `npm run build && npm run typecheck` zero-exit.
- `make ci` exits zero locally (full chain: fmt-check, legacy-surface-gate, verify-version-parity, lint, lint-feature-matrix, test-all, test-minimal, test-feature-matrix, test-surface-modularity, rmat-audit, audit).
- `xtask rmat-audit --strict` returns zero findings. Zero residual B-10-class violations.
- `xtask::machines::check_dsl_parity` + `verify-schema-freshness` both pass.
- Every git push uses the pre-push hook chain successfully — no `--no-verify` on commits landing in d.2-d.5.
- `dogma/wave-a-demolition` squash-merged onto `main`. Single commit message summarizing outcomes across waves a/b/c/d.
- GitHub `gate` workflow green on main after merge.
- Archive tag created: `archive/wave-a-demolition` preserving the full wave-a/b/c/d branch history for forensic reference.

**100% functional system** is the target. No `#[ignore]`-masked test failures. No architectural deferrals. No "we'll fix that later" — "later" within wave-d means "in the next commit", not "in a future wave".

---

## Section 7 — Out of scope (post-wave-d docs + release phase, NOT a "wave-e")

Wave-d explicitly does NOT include:

- **Version bump** — stays on 0.6.0. Release-time decision.
- **Doc regeneration from typed catalog** — auto-generated API docs from contracts + prose audit.
- **User-facing documentation audit** — `docs/guides/*.mdx` / `docs/reference/*.mdx` content stale against wave-c/d retypes.
- **CHANGELOG curation** — release-notes prose.
- **Tagged release** — `git tag v0.6.0` + push + `release.yml` workflow trigger.
- **Registry publishes** — 18 crates → crates.io, Python → PyPI, TypeScript → npm, Web → npm, GitHub release with binaries + checksums.

After wave-d ships a 100% green functional tree, the **docs + release phase** does those things. It is NOT called "wave-e". There is no wave-e. Any architectural item that feels like it wants to be wave-e work either (a) closes in wave-d now, or (b) is filed as a regular GitHub issue against a future 0.7.x / 1.0.0 release — never as a deferred wave.
