# semver-breaks report fixtures

Real `cargo-semver-checks` 0.48.0 output, captured from this repository. The
parser in `scripts/check_semver_breaks.py` is written against these bytes, not
against a guessed grammar.

| Fixture | Captured with | Notes |
|---|---|---|
| `report-meerkat-sqlite-0.8.22.txt` | `cargo semver-checks check-release -p meerkat-sqlite --baseline-version 0.8.22` | The four real 0.8.23 breaks in `meerkat-sqlite`. Four distinct lint ids, each with its own message shape. |
| `report-clean-two-crates.txt` | `cargo semver-checks check-release --workspace --release-type patch` (first two crates) | A clean run: `Summary no semver update required` plus `Finished`. |

Only one edit was applied to the captured bytes: in
`report-meerkat-sqlite-0.8.22.txt` the absolute worktree prefix
`/Users/<user>/.../meerkat/` was replaced with `/repo/`, so the fixture does not
carry a machine-specific path. The location suffix keeps its real shape
(`<absolute path>:<line>`), which is what the parser strips.
