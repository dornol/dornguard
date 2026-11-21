#!/bin/sh
set -eu

usage() {
    printf 'Usage: scripts/install.sh vVERSION [--with-deps]\n' >&2
    exit 2
}

version=
with_deps=false
for argument do
    case "$argument" in
        --with-deps) with_deps=true ;;
        -*|'') usage ;;
        *) [ -z "$version" ] || usage; version=$argument ;;
    esac
done
printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' || usage

kernel=$(uname -s)
case "$kernel:$(uname -m)" in
    Linux:x86_64) target=linux-x86_64 ;;
    Linux:aarch64)
        case "$version" in
            v0.0.4|v0.0.4-*|v0.0.5|v0.0.5-*|v0.0.6|v0.0.6-*|v0.0.7|v0.0.7-*)
                printf 'DornGuard %s publishes archives for Linux x86-64 and macOS ARM64.\n' "$version" >&2
                exit 2 ;;
            *) target=linux-arm64 ;;
        esac ;;
    Darwin:arm64) target=macos-arm64 ;;
    *) printf 'Unsupported DornGuard distribution target\n' >&2; exit 2 ;;
esac

ubuntu() (
    [ "$kernel" = Linux ] && [ -r /etc/os-release ] || exit 1
    . /etc/os-release
    [ "${ID:-}" = ubuntu ]
)

if "$with_deps" && ! ubuntu; then
    printf '%s\n' '--with-deps is available on Ubuntu, including Ubuntu WSL. Install engines separately on this platform.' >&2
    exit 2
fi

python_ready() {
    "$1" -I -c 'import sys, venv, ensurepip; sys.exit(not ((3, 10) <= sys.version_info[:2] <= (3, 14)))' \
        >/dev/null 2>&1
}

find_python() {
    for candidate in python3.14 python3.13 python3.12 python3.11 python3.10 python3 python; do
        if python_ready "$candidate"; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

jdk_ready() {
    if [ -n "$1" ]; then
        java="$1/bin/java"
        javac="$1/bin/javac"
    else
        java=java
        javac=javac
    fi
    command -v "$javac" >/dev/null 2>&1 || return 1
    java_version=$("$java" -version 2>&1) || return 1
    printf '%s\n' "$java_version" | awk -F '"' '/version "/ { major = $2 + 0; exit } END { exit !(major >= 21) }'
}

select_jdk() {
    engine_java_home=${JAVA_HOME:-}
    if jdk_ready "$engine_java_home"; then
        return 0
    fi
    for java_home in /usr/lib/jvm/java-21-openjdk-*; do
        if jdk_ready "$java_home"; then
            engine_java_home=$java_home
            return 0
        fi
    done
    return 1
}

bin_directory="${DORNGUARD_BIN_DIR:-$HOME/.local/bin}"
release_directory="${XDG_DATA_HOME:-$HOME/.local/share}/dornguard/releases"
destination="$release_directory/$version"
link="$bin_directory/dornguard"
if [ -e "$link" ] && [ ! -L "$link" ]; then
    printf 'An existing installation is at %s\n' "$link" >&2
    exit 2
fi

set --
if [ ! -d "$destination" ] || "$with_deps"; then
    command -v curl >/dev/null 2>&1 || set -- "$@" curl
    command -v unzip >/dev/null 2>&1 || set -- "$@" unzip
fi
if [ ! -d "$destination" ]; then
    command -v shasum >/dev/null 2>&1 || set -- "$@" libdigest-sha-perl
fi
if "$with_deps"; then
    if [ "${DORNGUARD_PYTHON+x}" = x ]; then
        engine_python=$DORNGUARD_PYTHON
        if ! python_ready "$engine_python"; then
            printf 'DORNGUARD_PYTHON must point to Python 3.10–3.14 with venv and ensurepip.\n' >&2
            exit 2
        fi
    elif ! engine_python=$(find_python); then
        set -- "$@" python3-venv
    fi
    select_jdk || set -- "$@" openjdk-21-jdk-headless
fi

if [ "$#" -gt 0 ]; then
    if "$with_deps"; then
        if [ "$(id -u)" -eq 0 ]; then
            apt-get update
            apt-get install -y "$@"
        else
            sudo apt-get update
            sudo apt-get install -y "$@"
        fi
    else
        printf 'DornGuard needs curl, unzip, and shasum to install a release.\n' >&2
        if ubuntu; then
            printf 'Install the missing packages with: sudo apt install %s\n' "$*" >&2
        fi
        exit 2
    fi
fi

if "$with_deps"; then
    if [ -z "$engine_python" ] && ! engine_python=$(find_python); then
        printf 'Python 3.10–3.14 with venv and ensurepip is required.\n' >&2
        exit 2
    fi
    if ! select_jdk; then
        printf 'JDK 21 or later is required. Set JAVA_HOME to its installation directory.\n' >&2
        exit 2
    fi
fi

mkdir -p "$bin_directory" "$release_directory"
release_directory=$(CDPATH='' cd -- "$release_directory" && pwd)
destination="$release_directory/$version"
archive="dornguard-$version-$target"
if [ ! -d "$destination" ]; then
    temporary=$(mktemp -d "$release_directory/.download.XXXXXX")
    trap 'rm -rf "$temporary"' EXIT
    base="https://github.com/dornol/dornguard/releases/download/$version"
    for suffix in .zip .zip.sha256; do
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
            --output "$temporary/$archive$suffix" "$base/$archive$suffix"
    done
    expected=$(awk 'NR == 1 { print $1 }' "$temporary/$archive.zip.sha256")
    actual=$(shasum -a 256 "$temporary/$archive.zip" | awk '{ print $1 }')
    if [ "$expected" != "$actual" ]; then
        printf 'DornGuard release checksum did not match\n' >&2
        exit 3
    fi
    unzip -q "$temporary/$archive.zip" -d "$temporary"
    test -f "$temporary/$archive/dornguard"
    test -f "$temporary/$archive/archunit-runner.jar"
    chmod 755 "$temporary/$archive/dornguard"
    mv "$temporary/$archive" "$destination"
fi

test -x "$destination/dornguard"
ln -sfn "$destination/dornguard" "$link"
printf 'Installed DornGuard %s at %s\n' "$version" "$link"

if "$with_deps"; then
    if [ "$engine_java_home" != "${JAVA_HOME:-}" ]; then
        printf 'Using JDK at %s. Set JAVA_HOME to this directory for subsequent checks.\n' "$engine_java_home"
        export JAVA_HOME="$engine_java_home"
    fi
    DORNGUARD_PYTHON="$engine_python" "$destination/dornguard" engines install
    (
        doctor_directory=$(mktemp -d)
        trap 'rm -rf "$doctor_directory"' EXIT
        cd "$doctor_directory"
        DORNGUARD_ARCHUNIT_RUNNER="$destination/archunit-runner.jar" \
            "$destination/dornguard" doctor
    )
fi
