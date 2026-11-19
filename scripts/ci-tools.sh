#!/bin/sh
set -eu

project_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
export DORNGUARD_ENGINE_DIR="${DORNGUARD_ENGINE_DIR:-$project_root/.ci-tools/managed}"
cd "$project_root"
cargo run --locked -- engines install
examples/spring-boot-multimodule/gradlew --no-daemon --console=plain -p runners/archunit-runner jar
