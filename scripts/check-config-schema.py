#!/usr/bin/env python3
"""Validate the configuration schema and its Gradle and Maven examples."""

import json
from pathlib import Path

from jsonschema import Draft202012Validator
import yaml


def main():
    root = Path(__file__).resolve().parent.parent
    schema = json.loads((root / "schemas/config.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    for example in (
        "examples/spring-boot-multimodule/.dornguard.yml",
        "schemas/examples/maven-layered.yml",
    ):
        document = yaml.safe_load((root / example).read_text(encoding="utf-8"))
        validator.validate(document)
        print(f"Validated {example}")


if __name__ == "__main__":
    main()
