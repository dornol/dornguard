#!/usr/bin/env python3
"""Check that install docs name the current release, or rewrite them with --write.

The release version comes from workspace.package.version in Cargo.toml and the
built-in ruleset version from BUILTIN_VERSION in dornguard-config. README.md
must mention only the current release. docs/releases.md must link the current
release notes and use the current version in its install command. Historical
mentions elsewhere in docs/releases.md (for example "v0.0.3 through v0.0.5")
are left alone.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VERSION = re.compile(r"\bv?\d+\.\d+\.\d+\b")


def read_versions():
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package = re.search(
        r'\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"', cargo, re.M | re.S
    )
    config = (ROOT / "crates/dornguard-config/src/lib.rs").read_text(encoding="utf-8")
    ruleset = re.search(r'BUILTIN_VERSION: &str = "([^"]+)"', config)
    if not package or not ruleset:
        sys.exit("Could not read the release or built-in ruleset version")
    return package.group(1), ruleset.group(1)


def expected_line(line, version, ruleset):
    """Return the line with every release and ruleset reference made current."""
    if "built-in ruleset" in line or "ruleset.version" in line:
        line = re.sub(r"(built-in ruleset )\d+\.\d+\.\d+", rf"\g<1>{ruleset}", line)
        line = re.sub(r'"\d+\.\d+\.\d+"', f'"{ruleset}"', line)
        line = re.sub(r"(dornguard )\d+\.\d+\.\d+", rf"\g<1>{version}", line)
        return line
    return VERSION.sub(lambda m: ("v" if m.group(0).startswith("v") else "") + version, line)


def current_reference(path, line):
    """Whether a line is a current-release reference that must track Cargo.toml."""
    if path == "README.md":
        return bool(VERSION.search(line))
    return bool(
        re.search(r"See the \[v\d+\.\d+\.\d+ release notes\]|install\.sh v\d+\.\d+\.\d+", line)
    )


def main():
    write = "--write" in sys.argv[1:]
    version, ruleset = read_versions()
    problems = []
    for name in ("README.md", "docs/releases.md"):
        path = ROOT / name
        lines = path.read_text(encoding="utf-8").split("\n")
        changed = False
        for number, line in enumerate(lines, 1):
            if not current_reference(name, line):
                continue
            fixed = expected_line(line, version, ruleset)
            if name == "docs/releases.md":
                fixed = re.sub(r"releases/v\d+\.\d+\.\d+\.md", f"releases/v{version}.md", fixed)
            if fixed != line:
                problems.append(f"{name}:{number}: expected {fixed.strip()!r}")
                lines[number - 1] = fixed
                changed = True
        if write and changed:
            path.write_text("\n".join(lines), encoding="utf-8")
    notes = ROOT / f"docs/releases/v{version}.md"
    if not notes.is_file():
        problems.append(f"missing release notes {notes.relative_to(ROOT)}")
    if problems and not write:
        print("\n".join(problems), file=sys.stderr)
        sys.exit("Version references are out of date; run scripts/check-version-refs.py --write")
    print(f"Version references match v{version} (ruleset {ruleset})")


if __name__ == "__main__":
    main()
