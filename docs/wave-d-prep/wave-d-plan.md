# Wave-d plan — SDK prep? No. "Get it working first."

## Scope

Wave-d closes the gap between "wave-c spine landed" and "workspace compiles + all tests pass + `make ci` zero". Explicitly **narrow** per user direction: no version bump, no doc regen, no release rollout, no SDK package version sync. First release of 0.6.0 ships later under its own banner — wave-d is purely "make it work, cleanly, at 100% dogma correctness."

## What wave-d explicitly is

- Structural dogma closure — the 8 audit findings from the post-wave-c machine survey
- Compile-debt closure — meerkat-mob cascade tail, Track-B producer wiring, any per-crate residue
- Fmt + clippy sweep — end of `--no-verify` tolerance
- Test lanes — unit, int, e2e-fast/system/smoke/live/models all green
- `make ci` zero locally + GitHub gate green
- Squash-merge `dogma/wave-a-demolition` → `main`

## What wave-d explicitly is NOT

- Version bump (stays on 0.6.0; no release yet)
- SDK package version sync (Python / TypeScript / web)
- Doc regeneration from typed catalog
- User-facing documentation audit
- Tagged release, crates.io / PyPI / npm publishes
- CHANGELOG curation
- Any new feature work

All of those are post-wave-d. Wave-d's success criterion is: a green tree ready to release, not released.

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
- **#53 · D-tests-smoke e2e-smoke green** — `cargo e2e-smoke` (live-provider lane) zero-fail. This lane hits real APIs; flaky-ish, but user-specified as a wave-c gate and carried into wave-d. If hits are genuinely external (rate-limit, API drift), document + retry; don't mask with `#[ignore]`.
- **#54 · D-tests-models e2e-models green** — `cargo e2e-models` (per-model catalog validation). Opportunistic; flag if genuinely blocked by external API changes.

### d.4 — CI gate + merge

- **#55 · D-ci-green `make ci` zero locally** — the full CI chain (fmt-check, legacy-surface-gate, verify-version-parity, lint, lint-feature-matrix, test-all, test-minimal, test-feature-matrix, test-surface-modularity, rmat-audit, audit) passes locally without `--no-verify`.
- **#56 · D-merge squash-merge into main + GitHub gate green** — `git checkout main && git merge --squash dogma/wave-a-demolition`, single commit message summarizing waves a/b/c/d outcomes. Push. GitHub `gate` workflow green on the merge commit.

Task count: **8 (d.0) + 3 (d.1) + 2 (d.2) + 4 (d.3) + 2 (d.4) = 19 tasks**.

---

## Section 2 — Dependency graph

Serial spine:
```
d.0 (any task in phase — structural prerequisites for correctness invariants)
  ↓
d.1 (compile debt — needs d.0 done because some audit findings touch same files)
  ↓
d.2 (tree hygiene — needs d.1 done because fmt/clippy only matter when compile green)
  ↓
d.3 (test lanes — needs d.2 done because tests need compile + fmt + clippy green)
  ↓
d.4 (CI + merge — needs d.3 done)
```

Parallelism:
- **Within d.0**: 8 tasks largely independent (different machines / compositions / bridges). Can spawn 4-8 agents in parallel.
- **Within d.1**: #46 + #47 parallel (different files). #48 serializes on both.
- **Within d.2**: #49 + #50 parallel.
- **Within d.3**: Tests can fail-independently; assign test lanes to different agents for parallel debugging.
- **d.4 is serial** (single agent closing the final gate).

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

- **d.0 — Structural dogma closure.** 8 audit findings land as distinct commits (preferably one commit per finding for review clarity, not bundled). Exit gate: all 8 tasks completed; `xtask::machines::check_dsl_parity` passes; `rmat-audit --strict` zero findings; AuthMachine appears in ≥1 composition spec.
- **d.1 — Compile debt closure.** meerkat-mob lib + Track-B producer wiring + residual crate-level breakage. Exit gate: `cargo check --workspace --all-targets` zero-exit.
- **d.2 — Tree hygiene.** Fmt + clippy clean workspace-wide, no more `--no-verify` tolerance. Exit gate: `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` both zero-exit.
- **d.3 — Test lanes.** All lanes green. Exit gate: every named lane in the Makefile produces zero-fail.
- **d.4 — CI gate + merge.** `make ci` + squash-merge onto main + GitHub gate green. Exit gate: `main` is the new authoritative branch with all wave-a/b/c/d work.

---

## Section 5 — Risk register

1. **meerkat-mob cascade expands during #46 execution.** The 86-error cascade may resolve partially to reveal deeper architectural gaps (e.g., missing generated types from DSL regen). Mitigation: scope-cap at 500 LOC / 20 files; if blown, stop and report. Per wave-c experience, codegen regen often resolves ~60% of seemingly-separate errors.
2. **Track-B producer wiring (#47) crosses into new architectural integration.** Three seams — if any one requires new DSL state fields, widen scope per architectural-prerequisite rule; don't split into follow-ups.
3. **Live-provider test lanes flaky.** `e2e-smoke` and `e2e-live` hit external APIs. Mitigation: run under retry; if a specific provider is down, document retry-required but don't `#[ignore]` the test.
4. **Clippy findings hidden behind `--no-verify` for all of wave-c** may be substantial. Mitigation: triage into (a) real code smells to fix, (b) legitimate-per-context `#[allow]` with explanatory comment. Don't mass-allow.
5. **Fmt sweep introduces cross-worktree conflicts.** `cargo fmt --all` touches many files; if other agents have mid-work changes on the same files, merge conflicts. Mitigation: fmt sweep runs last in d.2 — after all d.1 work is merged.
6. **Squash-merge loses forensic detail.** User accepted squash per "wholesale rewrite of main" framing. The `dogma/wave-a-demolition` branch can be kept as a tag (`archive/wave-a-demolition`) for forensic access after the squash.

---

## Section 6 — Completion criteria

Wave-d is done when ALL hold:

- All 19 tasks (#38-#56) marked completed.
- `./scripts/repo-cargo check --workspace --all-targets` exits zero.
- `./scripts/repo-cargo nextest run --workspace` passes with zero failures.
- `./scripts/repo-cargo clippy --workspace -- -D warnings` passes zero-exit.
- `./scripts/repo-cargo fmt --all -- --check` exits zero.
- Every test lane green: `cargo unit`, `cargo int`, `cargo e2e-fast`, `cargo e2e-system`, `cargo e2e-smoke`, `cargo e2e-live`, `cargo e2e-models`.
- `make ci` exits zero locally (full chain: fmt-check, legacy-surface-gate, verify-version-parity, lint, lint-feature-matrix, test-all, test-minimal, test-feature-matrix, test-surface-modularity, rmat-audit, audit).
- `xtask rmat-audit --strict` returns zero findings. Zero residual B-10-class violations.
- `xtask::machines::check_dsl_parity` + `verify-schema-freshness` both pass.
- Every git push uses the pre-push hook chain successfully — no `--no-verify` on commits landing in d.2-d.4.
- `dogma/wave-a-demolition` squash-merged onto `main`. Single commit message summarizing outcomes across waves.
- GitHub `gate` workflow green on main after merge.
- Archive tag created: `archive/wave-a-demolition` preserving the full wave-a/b/c/d branch history for forensic reference.

---

## Section 7 — Out of scope (shipping-phase problems)

Explicitly deferred beyond wave-d:

- Version bump (0.6.0 → 0.7.0 or 1.0.0). Wave-d stays on 0.6.0.
- SDK package version sync (Python `pyproject.toml`, TypeScript `package.json`, Web `package.json`, `artifacts/schemas/version.json`, `ContractVersion::CURRENT`, 18 internal crate dep versions).
- Doc regeneration from typed catalog. Any `docs/guides/*.mdx` / `docs/reference/*.mdx` prose that's stale against wave-c retypes.
- Tagged release (`git tag v0.6.0` + push + `release.yml` workflow trigger).
- Registry publishes: 18 crates → crates.io, Python SDK → PyPI, TypeScript SDK → npm, GitHub release with binaries + checksums.
- CHANGELOG entry curation.
- Post-0.6.0 queue: issue #341 (fault-explicit state machines), #343 (signal-route dispatcher), any new residual issues surfaced during wave-d.

All of these become the post-wave-d phase, candidate for its own dedicated session.
