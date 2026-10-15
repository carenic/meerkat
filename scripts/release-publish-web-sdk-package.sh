#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
tarball="${1:-}"

if [[ -z "${tarball}" ]]; then
  tarball="$(find "${root}/web-sdk-package" "${root}/dist/web-sdk-package" -maxdepth 1 -name 'rkat-web-*.tgz' -print -quit 2>/dev/null || true)"
fi

if [[ -z "${tarball}" || ! -f "${tarball}" ]]; then
  echo "Usage: $0 /path/to/rkat-web-<version>.tgz" >&2
  exit 2
fi

tarball="$(cd "$(dirname "${tarball}")" && pwd -P)/$(basename "${tarball}")"

cd "${root}"

dry_run="${MEERKAT_REGISTRY_DRY_RUN:-${REGISTRY_DRY_RUN:-false}}"
package_spec="$(node -p "const p = require('./sdks/web/package.json'); p.name + '@' + p.version")"

# The tarball is what npm publishes: its own package.json must name this
# checkout's package and the release tag's version.
release_version="${VERSION:-${RELEASE_TAG:-}}"
release_version="${release_version#v}"
if [[ -z "${release_version}" ]]; then
  echo "VERSION or RELEASE_TAG must name the release tag being published" >&2
  exit 2
fi
tarball_spec="$(tar -xzOf "${tarball}" package/package.json \
  | node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
      const p = JSON.parse(s);
      console.log(p.name + "@" + p.version);
    });')"
if [[ "${tarball_spec}" != "${package_spec}" ]]; then
  echo "tarball package ${tarball_spec} is not this checkout's ${package_spec}" >&2
  exit 1
fi
if [[ "${tarball_spec##*@}" != "${release_version}" ]]; then
  echo "tarball version ${tarball_spec##*@} does not match the release tag version ${release_version}" >&2
  exit 1
fi

if [[ "${dry_run}" != "true" ]] && npm view "${package_spec}" version >/dev/null 2>&1; then
  echo "${package_spec} already published, skipping"
  exit 0
fi

if [[ -n "${NODE_AUTH_TOKEN:-}" ]]; then
  npm config set //registry.npmjs.org/:_authToken "${NODE_AUTH_TOKEN}"
fi

if [[ "${dry_run}" == "true" ]]; then
  npm publish "${tarball}" --access public --dry-run --ignore-scripts
else
  npm publish "${tarball}" --access public --ignore-scripts
fi
