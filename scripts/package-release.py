#!/usr/bin/env python3
"""Bundle the matching JVM artifacts with a release build."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
RUNNER = ROOT / "runners/archunit-runner/build/libs/archunit-runner.jar"


def smoke_test(archive_path, name, executable):
    environment = os.environ.copy()
    environment.pop("DORNGUARD_ARCHUNIT_RUNNER", None)
    with tempfile.TemporaryDirectory(prefix="dornguard release ") as temporary:
        with zipfile.ZipFile(archive_path) as archive:
            archive.extractall(temporary)
        binary = Path(temporary) / name / executable
        if os.name != "nt":
            binary.chmod(0o755)
        example = ROOT / "examples/spring-boot-multimodule"
        result = subprocess.run(
            [str(binary), "check", str(example), "--format", "json"],
            env=environment, stdout=subprocess.PIPE,
        )
        report = json.loads(result.stdout)
        if (result.returncode != 0 or report["status"] != "passed"
                or not report["complete"] or not report["files_scanned"]):
            raise RuntimeError(f"Packaged CLI returned an incomplete report: {report}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=["linux-x86_64", "macos-arm64"])
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--tag")
    parser.add_argument("--smoke-test", action="store_true")
    args = parser.parse_args()

    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["workspace"]["package"]["version"]
    if args.tag and args.tag != f"v{version}":
        parser.error(f"tag {args.tag} does not match Cargo version {version}")

    executable = "dornguard"
    args.output.mkdir(parents=True, exist_ok=True)
    name = f"dornguard-v{version}-{args.platform}"
    archive_path = args.output / f"{name}.zip"
    with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for asset in (ROOT / "target/release" / executable, RUNNER, RUNNER.with_name("maven-model.jar"),
                      ROOT / "README.md", ROOT / "LICENSE",
                      ROOT / "THIRD_PARTY_NOTICES.md", ROOT / "licenses/Apache-2.0.txt"):
            archive.write(asset, f"{name}/{asset.name}")
    with archive_path.open("rb") as archive:
        digest = hashlib.file_digest(archive, "sha256").hexdigest()
    if args.smoke_test:
        smoke_test(archive_path, name, executable)
    archive_path.with_suffix(".zip.sha256").write_text(f"{digest}  {archive_path.name}\n", encoding="utf-8")
    print(archive_path)


if __name__ == "__main__":
    main()
