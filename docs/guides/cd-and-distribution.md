---
title: "CD and distribution"
description: "How Meerkat release artifacts are validated, built, and published across Rust, binaries, Python, and TypeScript."
icon: "truck-fast"
---

Meerkat publishes one source project across several consumer surfaces:

| Artifact | Audience | Published as |
|----------|----------|--------------|
| Rust crates | Rust library users and surface binaries | crates.io |
| `rkat` | CLI users | Homebrew tap for macOS/Linux, GitHub Release binary, Rust crate binary |
| `rkat-rpc` | SDK backends and JSON-RPC hosts | GitHub Release binary |
| `rkat-rest` | HTTP/SSE service hosts | GitHub Release binary |
| `rkat-mcp` | MCP host integrations | GitHub Release binary |
| Python SDK | Python applications | `meerkat-sdk` on PyPI |
| TypeScript SDK | Node applications | `@rkat/sdk` on npm |
| Web SDK | Browser applications | `@rkat/web` on npm |

The public release path is GitHub Actions. BuildBuddy release lanes are an
owner-only acceleration path and use the same Make-level contract.

## Versioning and Compatibility

Meerkat is pre-1.0 and releases on a fast `0.x.y` patch train. The policy,
stated plainly so downstream embedders can build against it:

- **Patch releases may change public APIs.** A `0.7.x` → `0.7.(x+1)` bump can
  add required fields, change function signatures, or remove items. Cargo's
  default caret requirement (`meerkat = "0.7"`) treats the whole `0.7`
  family as compatible, which is stronger than this project guarantees.
- **Embedders must pin exact versions.** Libraries and applications that
  build against Meerkat crates should declare `=0.7.19`-style exact pins and
  move deliberately, reading the changelog for each hop.
- **The only supported crate combination is exact version parity.** All
  workspace crates (`meerkat`, `meerkat-core`, `meerkat-runtime`, …), the
  Python/TypeScript/Web SDKs, and `ContractVersion::CURRENT` are lock-stepped
  to one version per release. Mixing crate versions across releases is
  unsupported.
- **Breaking API changes are flagged in the changelog.** Public-signature
  changes land under a `### Breaking` heading in `CHANGELOG.md` for the
  release that ships them; observable default-behavior changes land under
  `### Changed`. A release with neither heading is intended to be a drop-in
  replacement for the previous patch version.

### Downstream compatibility matrix

Known downstream projects that embed Meerkat, and the exact versions they
were built and verified against:

| Downstream | Downstream version | Meerkat version |
|------------|--------------------|-----------------|
| meerkat-mobkit | 0.7.22 | =0.7.15 |
| meerkat-mobkit | 0.7.23 | =0.7.17 |

Downstream projects should declare their supported Meerkat version as an
exact pin in their own `Cargo.toml` (not only in `Cargo.lock`), so consumers
can read the supported combination without archaeologizing lockfiles at
release tags.

## Release Checks

Run the release gate before cutting a tag:

```bash
make release-preflight
make verify-version-parity
make verify-schema-freshness
make release-dry-run
```

These checks keep the Rust workspace version, Python package version,
TypeScript package version, and generated contract artifacts aligned before any
registry publish happens.

## Declared Breaks (`semver-breaks`)

0.x patch releases may break public API; every break must be declared. The
`semver-breaks` gate runs cargo-semver-checks over the publishable workspace
against the published crates.io baselines and fails the release unless all
three hold:

1. **Measured.** The run reached every crate the release publishes, and its
   exit code agrees with its content. A run that died halfway, or failed for a
   reason other than a detected break, is a failure rather than a pass.
2. **Named.** Every finding the tool reports is named in the pending release
   section's `### Breaking` body, at the granularity of the finding. A type
   gaining a field and the same type losing a derive are two findings; naming
   one does not declare the other.
3. **Stamped.** The pending section is stamped `## [VERSION] - DATE` against
   the version being released. Notes still sitting under `## [Unreleased]`
   after the version bump has landed would publish as release notes titled
   "Unreleased", and that is a failure.

Behaviour-only breaks - a public signature that keeps its shape and changes
what it does - are invisible to cargo-semver-checks and therefore to this gate.
Declare them by hand in `### Breaking`.

Where it runs:

| Lane | Entry point |
|------|-------------|
| Local preflight | `make semver-breaks` (part of `make release-preflight`) |
| Release workflow | job `release_semver_gate`, required by `publish_github_release`, `publish_registries`, and `publish_unix_release_and_homebrew` |
| Every PR | `make semver-breaks-selftest` in the CI `ratchets` job: unit-tests the report parser against committed real reports, without needing cargo-semver-checks installed |

The judgement lives in `scripts/check_semver_breaks.py`, which is a pure
function of (report, changelog, version, tool exit code) and has no environment
override that relaxes it. `scripts/check-semver-breaks.sh` only produces the
report and hands it over.

## Binary Artifacts

Release assets are built for these binaries:

- `rkat`
- `rkat-rpc`
- `rkat-rest`
- `rkat-mcp`

Standard targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

Release assets include platform archives plus a checksum manifest:

- `checksums.sha256`
- `index.json`

## Homebrew Tap

The featured CLI install path is the Homebrew tap:

```bash
brew install lukacf/meerkat/rkat
```

The generated formula supports both macOS and Linux release assets. Linux users
should install Homebrew from the official
[Homebrew on Linux](https://docs.brew.sh/Homebrew-on-Linux) instructions before
using the same tap command.

The formula installs `rkat` plus the companion binaries:

- `rkat-rpc`
- `rkat-rest`
- `rkat-mcp`

Reduced distributions are source builds of the same crates with a narrower
feature set, not separate public binaries.

## SDK Bootstrap

Python and TypeScript SDK users should not need a local Rust toolchain.

| SDK | Install | Runtime resolution |
|-----|---------|--------------------|
| Python | `pip install meerkat-sdk` | Uses `MEERKAT_BIN_PATH` when set; otherwise resolves a matching `rkat-rpc` release binary |
| TypeScript | `npm install @rkat/sdk` | Uses an explicit binary path when configured; otherwise resolves a matching `rkat-rpc` release binary |

The SDKs are clients. They start or connect to the JSON-RPC surface rather than
embedding a separate runtime implementation.

## Release Workflow

1. Complete CI on the exact release commit and emit its tree-bound
   attestation.
2. Push the tag for that same commit; the tag gate consumes the attestation
   instead of recomputing CI.
3. Build platform binaries.
4. Create the GitHub release and upload binary assets.
5. Update the Homebrew tap formula.
6. Publish Rust crates.
7. Publish Python, TypeScript, and Web SDK packages.
8. Run install smoke checks for at least one platform.

The attestation binds the repository, commit SHA, Git tree SHA, CI workflow run
and attempt, branch, event, and aggregate Cargo result. The release gate
downloads it from the successful exact-main workflow run and verifies every
field before any publish job starts. Manual recovery dispatches run the
release-validation lane directly so releases older than the attestation
retention window remain repairable.

Manual release dispatch supports dry-run registry validation. Locally, use:

```bash
make release-workflow VERSION=vX.Y.Z REGISTRY_DRY_RUN=true
MEERKAT_BUILDBUDDY=1 make release-workflow VERSION=vX.Y.Z REGISTRY_DRY_RUN=true
```

## BuildBuddy

Cargo is the default backend. BuildBuddy is selected explicitly:

```bash
MEERKAT_BUILDBUDDY=1 make release-preflight
MEERKAT_BUILDBUDDY=1 make release-assets VERSION=vX.Y.Z
```

Use `make buildbuddy-doctor` when the local BuildBuddy setup looks suspicious.
It checks the API key, pinned `bb` CLI, generated Bazel files, selector
behavior, and lane isolation without printing secrets.

## Credentials

Registry credentials are independent:

| Registry | Credential |
|----------|------------|
| Homebrew tap | `HOMEBREW_TAP_TOKEN` |
| crates.io | Cargo publish token |
| PyPI | `PYPI_API_TOKEN` |
| npm | `NPM_TOKEN` |

Keep tokens in CI secrets or a local secret store. Do not commit registry
tokens, private BuildBuddy endpoints, or enterprise infrastructure names.

## Hard Rules

- Release only from tagged versions.
- Never publish mismatched Rust, Python, TypeScript, or contract versions.
- Never publish SDKs from a commit with stale generated schema artifacts.
- Keep public binary names stable: `rkat`, `rkat-rpc`, `rkat-rest`, `rkat-mcp`.
- Publish checksums and an index for release binary consumers.

## See Also

- [Build and CI](/reference/build-and-ci)
- [CLI commands](/cli/commands)
