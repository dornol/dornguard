#!/bin/sh
set -eu

project_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
export DORNGUARD_ENGINE_DIR="${DORNGUARD_ENGINE_DIR:-$project_root/.ci-tools/managed}"
cd "$project_root"
retry="$project_root/scripts/retry.sh"
"$retry" cargo run --locked -- engines install
"$retry" examples/spring-boot-multimodule/gradlew --no-daemon --console=plain -p runners/archunit-runner jar
mkdir -p "$project_root/.ci-tools/eslint"
cp scripts/ci-eslint/package.json scripts/ci-eslint/package-lock.json "$project_root/.ci-tools/eslint/"
(cd "$project_root/.ci-tools/eslint" && "$retry" npm ci --no-audit --no-fund)
