#!/usr/bin/env python3
"""Fail when a packaged release crate requires a sibling release crate inexactly.

Usage: check_packaged_internal_requirements.py VERSION PACKAGE_DIR CRATE...

Reads `PACKAGE_DIR/<crate>-VERSION.crate` for every CRATE and parses the
normalized `Cargo.toml` that `cargo package` wrote into it. Every dependency
on another CRATE (normal, build, dev and target tables; a renamed dependency
counts by its `package`) must require exactly `=VERSION`: meerkat 0.x patch
releases may break public API, so a published crate must resolve exactly the
sibling crates it was released with. scripts/check_rust_release_packaging.py
holds the workspace manifests to the same rule; this checks what ships.

Exit 0 = every packaged requirement on a release crate is exact, 1 = one is
not, 2 = usage error or an archive or manifest that cannot be read.
"""

from __future__ import annotations

import pathlib
import sys
import tarfile

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised only on Python < 3.11
    import tomli as tomllib

DEPENDENCY_TABLES = ("dependencies", "build-dependencies", "dev-dependencies")


def dependency_tables(manifest: dict):
    for table in DEPENDENCY_TABLES:
        yield table, manifest.get(table) or {}
    for target, target_tables in (manifest.get("target") or {}).items():
        for table in DEPENDENCY_TABLES:
            yield f"target.{target}.{table}", (target_tables or {}).get(table) or {}


def packaged_manifest(archive: pathlib.Path, member: str) -> dict:
    with tarfile.open(archive, "r:gz") as bundle:
        handle = bundle.extractfile(member)
        if handle is None:
            raise KeyError(member)
        return tomllib.loads(handle.read().decode("utf-8"))


def main(argv: list[str]) -> int:
    if len(argv) < 4:
        print(
            "usage: check_packaged_internal_requirements.py VERSION PACKAGE_DIR CRATE...",
            file=sys.stderr,
        )
        return 2

    version, package_dir, crates = argv[1], pathlib.Path(argv[2]), argv[3:]
    release_crates = set(crates)
    exact = f"={version}"
    errors: list[str] = []
    for crate in crates:
        archive = package_dir / f"{crate}-{version}.crate"
        member = f"{crate}-{version}/Cargo.toml"
        try:
            manifest = packaged_manifest(archive, member)
        except (
            OSError,
            KeyError,
            tarfile.TarError,
            UnicodeDecodeError,
            tomllib.TOMLDecodeError,
        ) as error:
            print(f"error: cannot read {member} from {archive}: {error}", file=sys.stderr)
            return 2
        for table, deps in dependency_tables(manifest):
            for key, spec in deps.items():
                package = spec.get("package", key) if isinstance(spec, dict) else key
                if package not in release_crates:
                    continue
                requirement = spec.get("version") if isinstance(spec, dict) else spec
                if requirement != exact:
                    errors.append(
                        f"{archive.name}: [{table}] `{key}` requires `{requirement}`, "
                        f"not `{exact}`"
                    )

    if errors:
        print(
            "Packaged release crates require a sibling release crate inexactly:",
            file=sys.stderr,
        )
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print(
        f"Every packaged requirement on a release crate is exact ({exact}) "
        f"in {len(crates)} archives"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
