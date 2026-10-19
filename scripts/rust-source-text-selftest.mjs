#!/usr/bin/env node
// Self-test of scripts/rust-source-text.mjs: comments are removed, every kind
// of literal survives, and namesRepoScripts, the BUILD generator's
// //:repo_governance_files heuristic, ignores a script named only in prose.

import assert from "node:assert/strict";
import { namesRepoScripts, stripRustComments } from "./rust-source-text.mjs";

// [source, names a script in code]
const CASES = [
  // Prose mentions: comments of every kind.
  ["// The workspace root: `MEERKAT_WORKSPACE_ROOT` (scripts/repo-cargo)\nfn a() {}", false],
  ["/// Run with ./scripts/repo-cargo test\nfn a() {}", false],
  ["//! python3 scripts/analyze-native-cost.py\n", false],
  ["/* scripts/x */ fn a() {}", false],
  ["/** see scripts/x */ fn a() {}", false],
  ["/* outer /* scripts/x */ still a comment */ fn a() {}", false],
  ["/* outer /* inner */ scripts/x */ fn a() {}", false],
  ["code(); // scripts/x", false],
  ["/* a\n * scripts/x\n */\nfn a() {}", false],
  // Code mentions: strings of every kind.
  ['let path = repo_file("scripts/repo-cargo");', true],
  ['let s = "// not a comment, scripts/x";', true],
  ['let s = "/* not a comment */ scripts/x";', true],
  ['let s = "escaped \\" quote // scripts/x";', true],
  ['let s = r"scripts/x";', true],
  ['let s = r#"/* not a comment */ scripts/x"#;', true],
  ['let s = r##"a "# inside, scripts/x"##;', true],
  ['let s = b"scripts/x";', true],
  ['let s = br#"scripts/x"#;', true],
  // A raw string may hold a quote; the comment after it is still a comment.
  ['let s = r#"a"b"#; // scripts/x', false],
  ['let s = "first line\n// still the string\nscripts/x";', true],
  // A literal that holds a quote or a comment opener does not swallow what
  // follows it.
  ["let c = '\"'; // scripts/x", false],
  ["let c = b'\"'; // scripts/x", false],
  ["let c = '\\''; // scripts/x", false],
  ["let c = '/'; let d = '*'; // scripts/x", false],
  // Lifetimes and labels are not char literals.
  ["fn f<'a>(x: &'a str) -> &'static str { /* scripts/x */ x }", false],
  ["'outer: loop { break 'outer; } // scripts/x", false],
  // A raw identifier is not a raw string.
  ["let r#type = 1; // scripts/x", false],
  // Division is not a comment.
  ['let half = total / 2; let p = "scripts/x";', true],
  // No mention at all.
  ["fn transcripts() {}", false],
];

for (const [source, expected] of CASES) {
  assert.equal(namesRepoScripts(source), expected, `namesRepoScripts(${JSON.stringify(source)})`);
}

// Comments leave their newlines, and the tokens around them stay apart.
assert.equal(stripRustComments("a /* x\ny */ b // c\nd"), "a \n b  \nd");
assert.equal(stripRustComments("a/*x*/b"), "a b");
// An unterminated comment or string ends at the end of the source.
assert.equal(stripRustComments("a /* never closed"), "a  ");
assert.equal(stripRustComments('a "never closed // x'), 'a "never closed // x');

console.log("rust-source-text contracts hold");
