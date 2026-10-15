#!/usr/bin/env node
// Fail-closed release-projection classifier: does <head> differ from <base>
// only by the version projection scripts/release-hook.sh writes for a
// cargo-release version bump?
//
// Exit status: 0 projection only, 1 not a projection (or not provably one),
// 2 classification error. One implementation serves the pre-push hooks
// (scripts/pre-push-unit.sh, pre-push-ratchets.sh, pre-push-machines.sh) and
// CI (scripts/ci-cargo-lanes.mjs and scripts/ci-release-evidence.mjs, which
// the CI gate and the release workflow's require_ci_green run), so the hook
// and CI accept exactly the same diffs.
//
// Every changed path must be a modification of a projection surface, and
// every changed byte must be a version substitution:
//   - Cargo.toml: [workspace.package] version and the versions of
//     [workspace.dependencies] path entries;
//   - Cargo.lock and standalone example locks (examples/<name>/Cargo.lock,
//     refreshed by scripts/example-locks.py): versions of source-less (path)
//     packages only;
//   - the documentation the docs stamper (scripts/stamp-docs-contract-version.sh)
//     owns: an old-version token may become the new version, every other
//     byte stays identical, so historical mentions of either version are
//     kept verbatim;
//   - every other projection surface (SDK manifests and generated types,
//     schemas, BUILD.bazel, ContractVersion::CURRENT): every occurrence of
//     the old version becomes the new one;
//   - CHANGELOG.md must carry the new release heading; MODULE.bazel.lock is
//     proven by its own freshness gate.

import { spawnSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";

// CHANGELOG.md alone is over 1.6 MiB; spawnSync's 1 MiB default buffer made
// `git show` fail on it.
const GIT_MAX_BUFFER = 1024 * 1024 * 1024;

export class ReleaseProjectionError extends Error {}

class NotProjection extends Error {}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function versionParts(version) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/);
  if (!match) return null;
  return {
    major: match[1],
    minor: match[2],
    patch: match[3],
    prerelease: match[4] ?? "",
  };
}

function normalizeStructuredContractVersion(text, parts) {
  const pattern = new RegExp(
    `(\\"contract_version\\"\\s*:\\s*\\{\\s*\\"major\\"\\s*:\\s*)${escapeRegExp(parts.major)}` +
      `(\\s*,\\s*\\"minor\\"\\s*:\\s*)${escapeRegExp(parts.minor)}` +
      `(\\s*,\\s*\\"patch\\"\\s*:\\s*)${escapeRegExp(parts.patch)}`,
    "g",
  );
  return text.replace(pattern, "$1@@MAJOR@@$2@@MINOR@@$3@@PATCH@@");
}

function normalizeContractSource(text, parts) {
  const current = /(pub const CURRENT:[\s\S]*?=\s*Self\s*\{)([\s\S]*?)(\n\s*\};)/;
  text = text.replace(current, (_all, prefix, body, suffix) => {
    const normalized = body
      .replace(new RegExp(`(major:\\s*)${escapeRegExp(parts.major)}\\b`), "$1@@MAJOR@@")
      .replace(new RegExp(`(minor:\\s*)${escapeRegExp(parts.minor)}\\b`), "$1@@MINOR@@")
      .replace(new RegExp(`(patch:\\s*)${escapeRegExp(parts.patch)}\\b`), "$1@@PATCH@@");
    return `${prefix}${normalized}${suffix}`;
  });
  const prerelease = parts.prerelease
    ? `Some("${parts.prerelease}")`
    : "None";
  return text.replace(
    new RegExp(`(pub const PRERELEASE: Option<&'static str> = )${escapeRegExp(prerelease)};`),
    "$1@@PRERELEASE@@;",
  );
}

function normalizeCargoManifest(text, version) {
  let section = "";
  return text
    .split("\n")
    .map((line) => {
      const header = line.match(/^\s*\[([^\]]+)\]\s*$/);
      if (header) section = header[1];
      if (section === "workspace.package") {
        return line.replace(
          new RegExp(`^(\\s*version\\s*=\\s*\")${escapeRegExp(version)}(\"\\s*)$`),
          "$1@@MEERKAT_RELEASE_VERSION@@$2",
        );
      }
      if (section === "workspace.dependencies" && line.includes("path =")) {
        return line.replace(
          new RegExp(`(\\bversion\\s*=\\s*\")${escapeRegExp(version)}(\")`),
          "$1@@MEERKAT_RELEASE_VERSION@@$2",
        );
      }
      return line;
    })
    .join("\n");
}

function normalizeCargoLock(text, version) {
  return text
    .split(/(?=\[\[package\]\]\n)/)
    .map((block) => {
      if (!block.startsWith("[[package]]\n") || /^source\s*=/m.test(block)) {
        return block;
      }
      return block.replace(
        new RegExp(`^(version\\s*=\\s*\")${escapeRegExp(version)}(\"\\s*)$`, "m"),
        "$1@@MEERKAT_RELEASE_VERSION@@$2",
      );
    })
    .join("");
}

function isCargoLockPath(path) {
  return path === "Cargo.lock" || /^examples\/[^/]+\/Cargo\.lock$/.test(path);
}

// The files scripts/stamp-docs-contract-version.sh rewrites: Markdown and
// MDX under docs/ (never the independently released MobKit mirror or the
// unpublished docs/internal notes), the README and the platform skill. Their
// prose also names past and upcoming releases ("0.9.0 accepted only IP
// literals", "since 0.9.1"), which the stamper leaves alone.
function isStampedDocument(path) {
  if (path === "README.md" || path === ".claude/skills/meerkat-platform/SKILL.md") return true;
  return (
    path.startsWith("docs/") &&
    !path.startsWith("docs/mobkit/") &&
    !path.startsWith("docs/internal/") &&
    /\.mdx?$/.test(path)
  );
}

// A version token: the version not inside a longer dotted number or a
// prerelease/build-suffixed version ("10.9.0", "0.9.0.1", "0.9.0-rc.1").
function versionTokenPattern(version) {
  return new RegExp(
    `(?<![0-9.])${escapeRegExp(version)}(?![0-9]|\\.[0-9]|[-+][0-9A-Za-z])`,
    "g",
  );
}

// True when `after` is `before` with some old-version tokens replaced by the
// new version and every other byte identical. Each token of `before` admits
// either version, so the alignment tracks every reachable offset in `after`
// (one version may be a prefix of the other).
export function isVersionTokenSubstitution(before, after, oldVersion, newVersion) {
  const segments = [];
  let last = 0;
  for (const match of before.matchAll(versionTokenPattern(oldVersion))) {
    segments.push(before.slice(last, match.index));
    last = match.index + match[0].length;
  }
  segments.push(before.slice(last));
  let offsets = new Set([0]);
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index];
    const afterSegment = new Set();
    for (const offset of offsets) {
      if (after.startsWith(segment, offset)) afterSegment.add(offset + segment.length);
    }
    if (index === segments.length - 1) return afterSegment.has(after.length);
    offsets = new Set();
    for (const offset of afterSegment) {
      if (after.startsWith(oldVersion, offset)) offsets.add(offset + oldVersion.length);
      if (after.startsWith(newVersion, offset)) offsets.add(offset + newVersion.length);
    }
    if (offsets.size === 0) return false;
  }
  return false;
}

function normalizeProjection(path, text, version, parts) {
  let normalized;
  if (path === "Cargo.toml") {
    normalized = normalizeCargoManifest(text, version);
  } else if (isCargoLockPath(path)) {
    normalized = normalizeCargoLock(text, version);
  } else {
    normalized = text.split(version).join("@@MEERKAT_RELEASE_VERSION@@");
  }
  normalized = normalizeStructuredContractVersion(normalized, parts);
  if (path === "crates/meerkat-contracts/src/version.rs") {
    normalized = normalizeContractSource(normalized, parts);
  }
  return normalized;
}

function isProjectedPath(path) {
  return (
    path === "Cargo.toml" ||
    isCargoLockPath(path) ||
    path === "README.md" ||
    path === ".claude/skills/meerkat-platform/SKILL.md" ||
    path === "crates/meerkat-contracts/src/version.rs" ||
    path === "sdks/python/pyproject.toml" ||
    path === "sdks/typescript/package.json" ||
    path === "sdks/web/package.json" ||
    path === "sdks/web/src/runtime.ts" ||
    path.startsWith("sdks/web/src/generated/") ||
    path.startsWith("sdks/python/meerkat/generated/") ||
    path.startsWith("sdks/typescript/src/generated/") ||
    path.startsWith("artifacts/schemas/") ||
    (path.startsWith("docs/") && !path.startsWith("docs/mobkit/")) ||
    path.endsWith("/BUILD.bazel") ||
    path === "BUILD.bazel"
  );
}

function projectionMatches(path, before, after, versions) {
  const { oldVersion, newVersion, oldParts, newParts } = versions;
  if (isStampedDocument(path)) {
    // Struct-form contract versions in docs are always restamped; dotted
    // versions are aligned token by token.
    return isVersionTokenSubstitution(
      normalizeStructuredContractVersion(before, oldParts),
      normalizeStructuredContractVersion(after, newParts),
      oldVersion,
      newVersion,
    );
  }
  return (
    normalizeProjection(path, before, oldVersion, oldParts) ===
    normalizeProjection(path, after, newVersion, newParts)
  );
}

function makeGit(cwd) {
  const run = (gitArgs, { allowFailure = false } = {}) => {
    const result = spawnSync("git", gitArgs, { cwd, encoding: "utf8", maxBuffer: GIT_MAX_BUFFER });
    if (result.status !== 0) {
      if (allowFailure) return null;
      const detail = (result.stderr ?? "").trim() || result.error?.message || "";
      throw new ReleaseProjectionError(`git ${gitArgs.join(" ")} failed${detail ? `: ${detail}` : ""}`);
    }
    return result.stdout;
  };
  const succeeds = (gitArgs) => spawnSync("git", gitArgs, { cwd, stdio: "ignore" }).status === 0;
  return { run, succeeds };
}

function workspaceVersion(git, revision) {
  const manifest = git.run(["show", `${revision}:Cargo.toml`], { allowFailure: true });
  if (manifest === null) return null;
  const section = manifest.match(/(?:^|\n)\[workspace\.package\]\s*\n([\s\S]*?)(?=\n\[|$)/);
  if (!section) return null;
  const version = section[1].match(/^version\s*=\s*"([^"]+)"\s*$/m);
  return version?.[1] ?? null;
}

// Classify base..head. Returns { projection: true, oldVersion, newVersion,
// paths } for a release projection, { projection: false, reason } otherwise;
// throws ReleaseProjectionError when git cannot answer.
export function classifyReleaseProjection({ base, head, cwd = process.cwd() }) {
  if (!base || !head) throw new ReleaseProjectionError("base and head revisions are required");
  const git = makeGit(cwd);
  const notProjection = (reason) => {
    throw new NotProjection(reason);
  };
  try {
    if (!git.succeeds(["merge-base", "--is-ancestor", base, head])) {
      notProjection("base is not an ancestor of head");
    }

    const oldVersion = workspaceVersion(git, base);
    const newVersion = workspaceVersion(git, head);
    const oldParts = oldVersion ? versionParts(oldVersion) : null;
    const newParts = newVersion ? versionParts(newVersion) : null;
    if (!oldVersion || !newVersion || !oldParts || !newParts) {
      notProjection("workspace package version is missing or not semantic");
    }
    if (oldVersion === newVersion) {
      notProjection("workspace package version did not change");
    }
    const versions = { oldVersion, newVersion, oldParts, newParts };

    const statusText = git.run(["diff", "--no-renames", "--name-status", "-z", base, head, "--"]);
    const fields = statusText.split("\0");
    const changes = [];
    for (let index = 0; index + 1 < fields.length; index += 2) {
      changes.push({ status: fields[index], path: fields[index + 1] });
    }

    if (changes.length === 0) notProjection("no tracked files changed");
    if (!changes.some(({ path }) => path === "Cargo.toml")) {
      notProjection("Cargo.toml did not change");
    }
    if (!changes.some(({ path }) => path === "CHANGELOG.md")) {
      notProjection("CHANGELOG.md did not change");
    }

    for (const { status, path } of changes) {
      if (status !== "M") notProjection(`${path} has non-modification status ${status}`);
      if (path === "CHANGELOG.md") continue;
      // MODULE.bazel.lock embeds content hashes for Cargo.toml, Cargo.lock, and
      // generated repository data. Its dedicated freshness gate proves the whole
      // lock against the candidate tree, so it is intentionally not normalized
      // here.
      if (path === "MODULE.bazel.lock") continue;
      if (!isProjectedPath(path)) notProjection(`${path} is not a release projection surface`);

      const before = git.run(["show", `${base}:${path}`], { allowFailure: true });
      const after = git.run(["show", `${head}:${path}`], { allowFailure: true });
      if (before === null || after === null) notProjection(`${path} is not present at both revisions`);
      if (!projectionMatches(path, before, after, versions)) {
        notProjection(`${path} contains changes beyond the version projection`);
      }
    }

    const changelog = git.run(["show", `${head}:CHANGELOG.md`]);
    if (!changelog.includes(`## [${newVersion}]`)) {
      notProjection(`CHANGELOG.md does not contain a ${newVersion} release heading`);
    }
    return { projection: true, oldVersion, newVersion, paths: changes.map(({ path }) => path) };
  } catch (error) {
    if (error instanceof NotProjection) return { projection: false, reason: error.message };
    throw error;
  }
}

function main(argv) {
  let base = "";
  let head = "";
  let verbose = false;
  for (let i = 0; i < argv.length; i += 1) {
    switch (argv[i]) {
      case "--base":
        base = argv[++i] ?? "";
        break;
      case "--head":
        head = argv[++i] ?? "";
        break;
      case "--verbose":
        verbose = true;
        break;
      case "-h":
      case "--help":
        console.log("usage: release-projection-only.mjs --base <rev> --head <rev> [--verbose]");
        return 0;
      default:
        console.error(`unknown argument: ${argv[i]}`);
        return 2;
    }
  }
  if (!base || !head) {
    console.error("error: --base and --head are required");
    return 2;
  }
  let result;
  try {
    result = classifyReleaseProjection({ base, head });
  } catch (error) {
    console.error(error?.message ?? String(error));
    return 2;
  }
  if (!result.projection) {
    if (verbose) console.error(`not a release projection: ${result.reason}`);
    return 1;
  }
  if (verbose) console.log(`release projection only: ${result.oldVersion} -> ${result.newVersion}`);
  return 0;
}

const invokedPath = process.argv[1] ? realpathSync(process.argv[1]) : "";
if (invokedPath === realpathSync(fileURLToPath(import.meta.url))) {
  process.exit(main(process.argv.slice(2)));
}
