#!/usr/bin/env python3
"""Bind an isolated archive checkout to the ms-package source under validation."""
import argparse
import json
import os
from pathlib import Path
import re
import tomllib


def reconcile(archive: Path, package: Path) -> list[str]:
    """Update only ms-package dependency declarations in integration copies."""
    version = tomllib.loads((package / "Cargo.toml").read_text())["package"]["version"]
    required = tomllib.loads((package / "Cargo.toml").read_text())["dependencies"]["archive-core"]["version"]
    changed = []
    has_core_alias = False
    declaration = re.compile(r"(?m)^([ \t]*(?:ms-package|package-core)[ \t]*=[ \t]*)([^\n]+)$")
    for manifest in sorted(archive.rglob("Cargo.toml")):
        if any(part in {"target", "vendor", ".git"} for part in manifest.relative_to(archive).parts):
            continue
        source = manifest.read_text()

        def replace(match):
            nonlocal has_core_alias
            value = tomllib.loads("dependency = " + match[2])["dependency"]
            if isinstance(value, dict) and value.get("workspace"):
                return match[0]
            if isinstance(value, str):
                value = {"version": value}
            if match[1].split("=")[0].strip() == "package-core":
                if value.get("package") != "caddy-archive-core":
                    return match[0]
                has_core_alias = True
                value["version"] = "=" + required.lstrip("=^~")
                return match[1] + "{ " + ", ".join(f"{key} = {json.dumps(item)}" for key, item in value.items()) + " }"
            value["version"] = "=" + version
            value["path"] = os.path.relpath(package, manifest.parent)
            fields = []
            for key, item in value.items():
                # Dependency fields use TOML strings, booleans and string arrays,
                # whose JSON encodings are also valid TOML values.
                fields.append(f"{key} = {json.dumps(item)}")
            return match[1] + "{ " + ", ".join(fields) + " }"

        updated = declaration.sub(replace, source)
        tomllib.loads(updated)
        if updated != source:
            manifest.write_text(updated)
            changed.append(str(manifest.relative_to(archive)))
    # Older archive checkouts pass their local core's public types directly to
    # ms-package. Unify the registry copy with that same compatible core, only
    # in this integration workspace. Newer cores use a separate compatible
    # package reader dependency and do not need this patch.
    workspace = archive / "Cargo.toml"
    archive_data = tomllib.loads(workspace.read_text())
    core = archive / "crates/archive-core/Cargo.toml"
    if core.exists():
        core_version = tomllib.loads(core.read_text())["package"]["version"]
        if isinstance(core_version, dict):
            core_version = archive_data["workspace"]["package"]["version"]
        if not has_core_alias and core_version.split(".")[:2] == required.lstrip("=^~").split(".")[:2]:
            existing = archive_data.get("patch", {}).get("crates-io", {}).get("caddy-archive-core")
            if existing is None:
                with workspace.open("a") as output:
                    output.write('\n[patch.crates-io.caddy-archive-core]\npath = "crates/archive-core"\n')
                changed.append("Cargo.toml")
            elif existing.get("path") != "crates/archive-core":
                raise ValueError("incompatible archive-core patch in integration workspace")
    return changed


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path, help="isolated integration checkout, modified in place")
    args = parser.parse_args()
    package = Path(__file__).resolve().parents[1]
    print(json.dumps({"updated_manifests": reconcile(args.archive.resolve(), package)}))
