// Rust source text helpers for the Bazel BUILD generator's runfile heuristics.
//
// The generator decides a test's `data` from what its source text names. A
// comment that mentions a path is not a read of it, so the heuristics that
// must not fire on prose match the code text that stripRustComments leaves.

const IDENTIFIER_CHAR = /[A-Za-z0-9_]/;
// The start of a raw string literal (`r"`, `r#"`, `br"`, `cr##"`, ...); the
// capture is its hashes.
const RAW_STRING_START = /(?:b|c)?r(#*)"/y;
// A char or byte char literal. A `'` that does not start one opens a lifetime
// or a loop label, which runs no further than its identifier.
const CHAR_LITERAL = /b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\}|[nrt\\0'"])|[^\\'\n\r])'/uy;

// Rust source text with its comments removed: line comments (`//`, `///`,
// `//!`) up to the end of their line, and block comments, nested ones and doc
// blocks included. String, raw string, byte string and char literals stay as
// written, so a comment opener inside a literal is not taken for a comment. A
// removed comment leaves a space, or the newlines it spanned, so the tokens
// around it stay apart.
export function stripRustComments(source) {
  const parts = [];
  let copyFrom = 0;
  let index = 0;
  const length = source.length;
  while (index < length) {
    const char = source[index];
    const next = source[index + 1];
    if (char === "/" && next === "/") {
      const end = source.indexOf("\n", index);
      parts.push(source.slice(copyFrom, index), " ");
      index = end === -1 ? length : end;
      copyFrom = index;
      continue;
    }
    if (char === "/" && next === "*") {
      let depth = 1;
      let cursor = index + 2;
      let newlines = "";
      while (cursor < length && depth > 0) {
        if (source[cursor] === "/" && source[cursor + 1] === "*") {
          depth += 1;
          cursor += 2;
        } else if (source[cursor] === "*" && source[cursor + 1] === "/") {
          depth -= 1;
          cursor += 2;
        } else {
          if (source[cursor] === "\n") newlines += "\n";
          cursor += 1;
        }
      }
      parts.push(source.slice(copyFrom, index), newlines || " ");
      index = cursor;
      copyFrom = index;
      continue;
    }
    const startsToken = index === 0 || !IDENTIFIER_CHAR.test(source[index - 1]);
    if (startsToken && (char === "r" || char === "b" || char === "c")) {
      RAW_STRING_START.lastIndex = index;
      const raw = RAW_STRING_START.exec(source);
      if (raw) {
        const terminator = `"${raw[1]}`;
        const end = source.indexOf(terminator, RAW_STRING_START.lastIndex);
        index = end === -1 ? length : end + terminator.length;
        continue;
      }
    }
    if (char === '"') {
      let cursor = index + 1;
      while (cursor < length && source[cursor] !== '"') {
        cursor += source[cursor] === "\\" ? 2 : 1;
      }
      index = Math.min(cursor + 1, length);
      continue;
    }
    if (char === "'" || (char === "b" && next === "'" && startsToken)) {
      CHAR_LITERAL.lastIndex = index;
      const literal = CHAR_LITERAL.exec(source);
      if (literal) {
        index += literal[0].length;
        continue;
      }
    }
    index += 1;
  }
  parts.push(source.slice(copyFrom));
  return parts.join("");
}

// Whether Rust source text names the repository's scripts directory in code,
// a string such as `"scripts/repo-cargo"`. A mention in a comment or a doc
// comment does not count: the test it describes reads no script.
export function namesRepoScripts(source) {
  return source.includes("scripts/") && stripRustComments(source).includes("scripts/");
}
