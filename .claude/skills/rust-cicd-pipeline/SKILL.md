---
name: rust-cicd-pipeline
description: |
  Set up a professional Rust CI/CD pipeline with pre-commit hooks, automated linting,
  testing, changelog management, and version control. This skill should be used when
  creating a new Rust project that needs CI/CD, when adding CI/CD to an existing project,
  or when troubleshooting pipeline issues. The pipeline follows the "Makefile as single
  source of truth" pattern and works without GitHub Actions for enterprise environments.
---

# Rust CI/CD Pipeline Setup

This skill provides a complete, production-ready CI/CD pipeline for Rust projects featuring:

- **Progressive Validation**: Auto-fix and generated-file sync on commit;
  secret, format, codegen, lock, Clippy, and deterministic repo gates on push
- **Makefile as Single Source of Truth**: Identical commands locally and in any CI system
- **No GitHub Actions Dependency**: Works with Jenkins, GitLab CI, or any CI runner
- **Version Consistency**: Automatic verification between tag and Cargo.toml

## Architecture Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│   Pre-commit    │     │    Any CI       │     │    Release      │
│     Hooks       │     │    System       │     │    Process      │
├─────────────────┤     ├─────────────────┤     ├─────────────────┤
│ On commit:      │     │ On push/PR:     │     │ On v* tag:      │
│ - cargo fmt fix │     │ - make lint     │     │ - Verify version│
│ - sync generated│     │ - make test     │     │ - Read notes    │
│   files         │     │ - make test-all │     │ - Build release │
│                 │     │ - make audit    │     │ - Tag artifacts │
│                 │     │                 │     │                 │
├─────────────────┤     └─────────────────┘     └─────────────────┘
│ On push:        │              │
│ - secrets/fmt   │              ▼
│ - changed clippy│     ┌─────────────────┐
│ - repo gates    │     │    Makefile     │
└─────────────────┘     │    Makefile     │
         │              │ (Single Source) │
         └──────────────┤ - test          │
                        │ - test-all      │
                        │ - lint          │
                        │ - fmt           │
                        │ - audit         │
                        │ - ci            │
                        └─────────────────┘
```

## Quick Setup

To set up a complete pipeline for a project:

### 1. Prerequisites

Ensure the project has:
- `Cargo.toml` with proper package metadata
- A `tests/` directory for integration tests (unit tests typically in `src/`)
- Rust toolchain installed via rustup

### 2. Install Required Tools

```bash
# Install cargo tools for CI
rustup component add clippy rustfmt
cargo install cargo-deny cargo-audit cargo-tarpaulin
```

### 3. Create Required Files

Create these files from the reference templates:

| File | Purpose |
|------|---------|
| `Makefile` | Test command definitions |
| `.pre-commit-config.yaml` | Git hooks configuration |
| `deny.toml` | Dependency auditing rules |
| `CHANGELOG.md` | Release notes |

### 4. Install Hooks

```bash
pip install pre-commit
make install-hooks
```

## Detailed Configuration

### Makefile Setup

The Makefile defines all test and lint commands. Customize for your project structure:

```makefile
CRATE_NAME := your_crate_name
```

Key targets:
- `make test`: Fast unit tests for pre-commit (~seconds)
- `make test-all`: Full test suite with all features
- `make lint`: Clippy linting with strict settings
- `make fmt`: Check and fix formatting
- `make audit`: Security audit with cargo-deny
- `make ci`: Full CI suite (lint + all tests + audit)
- `make install-hooks`: Install git hooks
- `make coverage`: Generate test coverage report

See `references/makefile-template.md` for the complete template.

### Optional BuildBuddy Backend In Meerkat

In this repository, Cargo remains the default Make backend. Agents and humans
should use the same Make targets locally and opt into BuildBuddy with one
environment variable when available:

```bash
make build
make check
make lint
make test

MEERKAT_BUILDBUDDY=1 make build
MEERKAT_BUILDBUDDY=1 make lint
MEERKAT_BUILDBUDDY=1 make test
```

The explicit BuildBuddy developer targets are `make buildbuddy-build`,
`make buildbuddy-check`, `make buildbuddy-clippy`, `make buildbuddy-test`,
`make buildbuddy-test-unit`, `make buildbuddy-test-int`,
`make buildbuddy-e2e-fast`, `make buildbuddy-e2e-system`,
`make buildbuddy-e2e-live`, and `make buildbuddy-e2e-smoke`. Use
`BUILDBUDDY_DRY_RUN=1` to inspect explicit BuildBuddy Make targets. Use
`make buildbuddy-doctor` before debugging credentials, the pinned `bb` CLI,
Bazel metadata freshness, selector behavior, or lane isolation.

### Pre-commit Configuration

The `.pre-commit-config.yaml` runs:

**On every commit**:
- Rust formatting auto-fix
- Bazel BUILD regeneration when Cargo metadata changes
- Meerkat dogma mirror synchronization when its sources change

**On push only**:
- Secret, YAML/TOML, merge-conflict, and large-file checks
- Rust formatting verification and changed-crate Clippy
- machine/codegen, lock, generated-header, and documentation contract gates
- the deterministic workspace unit, integration, and e2e gate

Customize hooks based on your project needs.

See `references/pre-commit-config.yaml` for the template.

### Portable CI Integration

For projects without GitHub Actions, use the same Makefile targets with another
CI system. Meerkat itself does use GitHub Actions; the examples below show that
the Make command surface is not coupled to it.

**Jenkins Pipeline Example:**
```groovy
pipeline {
    agent any
    stages {
        stage('CI') {
            steps {
                sh 'make ci'
            }
        }
    }
}
```

**GitLab CI Example:**
```yaml
ci:
  script:
    - make ci
```

**Any CI System:**
```bash
# Single command runs all checks
make ci
```

### Cargo Deny Configuration

The `deny.toml` file configures dependency auditing:

- **Licenses**: Whitelist allowed licenses
- **Bans**: Block problematic dependencies
- **Advisories**: Check for security vulnerabilities
- **Sources**: Restrict dependency sources

See `references/deny-config.toml` for the template.

### Changelog Format

Maintain a `CHANGELOG.md` with version sections:

```markdown
# Changelog

## [1.2.0] - 2024-12-10
### Added
- New feature X

### Fixed
- Bug in Y
```

See `references/changelog-format.md` for detailed formatting guidelines.

### Cargo.toml Configuration

Ensure your `Cargo.toml` includes:

```toml
[package]
name = "your-crate"
version = "1.0.0"  # Single source of truth

[features]
default = []
integration-tests = []

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
```

See `references/cargo-example.toml` for complete configuration.

## Release Process

For Meerkat, write pending notes under `## [Unreleased]`, run
`make release-preflight`, and use the configured `cargo release` workflow. Do
not bump every package or stamp the changelog by hand. The pre-release hook
binds the new version to the notes, bumps SDK/docs/contract versions,
regenerates schemas and wrappers, and stages those files in the version-bump
commit. The pushed tag then drives exact-main GitHub Actions publication.

See `references/release-process.md` for the current repository-specific steps.

## Troubleshooting

### Pre-commit hooks not running
```bash
pre-commit install
pre-commit install --hook-type pre-push
```

### Clippy warnings in CI but not locally
Use the repository's committed `rust-toolchain.toml` pin. The current pin is:
```toml
[toolchain]
channel = "1.94.1"
components = ["rustfmt", "clippy"]
```

### Cargo deny fails with unknown advisory
Update the advisory database:
```bash
cargo deny fetch
```

### Skip hooks temporarily
```bash
git commit --no-verify -m "message"
git push --no-verify
```

Do not bypass Meerkat's installed pre-push hook unless the repository owner
explicitly authorizes that exact push. The hook is part of the normal release
and publication evidence.

### Test features not found
Ensure feature flags are consistent between Cargo.toml and test commands.

## Test Categories and Policy

The pipeline uses test categories with clear boundaries:

| Category | Speed | I/O | Dependencies | When Run |
|----------|-------|-----|--------------|----------|
| **Unit** | Fast | Usually none | Focused components | Pre-push and CI |
| **Integration** | Moderate | Local fixtures | Real components, mocked external | Pre-push and CI |
| **E2E** | Minutes | Real or hermetic system | Full system | Named CI/manual lanes |

### Unit Tests (`src/` with `#[cfg(test)]`)

Keep pure logic tests isolated. Core tests that exercise bounded native config,
path, lock, or fence I/O use temporary fixtures rather than claiming zero I/O.

- **Purpose**: Verify individual functions/modules in isolation
- **Speed**: Must complete in <100ms per test, total suite <10s
- **Mocking**: Mock everything external (DB, APIs, file system)
- **Run**: `make test` (the repository's pre-push/CI command surface)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mockall::predicate::*;

    #[test]
    fn test_calculate_total() {
        // Pure logic test
        assert_eq!(calculate_total(&[10, 20, 30]), 60);
    }
}
```

### Integration Tests (`tests/`)

Test component interactions with mocked external services.

- **Purpose**: Verify components work together correctly
- **Speed**: <1s per test, real component wiring but mocked I/O
- **Run**: `make test-all` (pre-push, CI)

```rust
// tests/integration_test.rs
#[test]
fn test_user_workflow() {
    let mock_api = MockApiClient::new();
    let result = create_and_fetch_user(&mock_api, "test@example.com");
    assert_eq!(result.email, "test@example.com");
}
```

### Directory Structure

```
your-crate/
├── Cargo.toml
├── Makefile
├── .pre-commit-config.yaml
├── deny.toml
├── CHANGELOG.md
├── src/
│   ├── lib.rs           # With #[cfg(test)] mod tests
│   └── main.rs
├── tests/               # Integration tests
│   ├── integration.rs
│   └── common/
│       └── mod.rs       # Shared test utilities
└── benches/             # Benchmarks (optional)
    └── benchmark.rs
```

## Resources

This skill includes reference templates in the `references/` directory:

### references/

- **pre-commit-config.yaml**: Pre-commit hooks configuration template
- **makefile-template.md**: Makefile with all standard targets
- **deny-config.toml**: Cargo deny configuration
- **changelog-format.md**: Changelog formatting guide
- **cargo-example.toml**: Complete Cargo.toml configuration
- **release-process.md**: Step-by-step release instructions
- **rust-toolchain-example.toml**: Rust version pinning

To use: Read the relevant template, customize placeholders (marked with `{{PLACEHOLDER}}`), and save to your project.
