#!/bin/sh
set -eu

version="${1:-}"
if ! printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    printf 'Usage: scripts/install.sh vVERSION\n' >&2
    exit 2
fi

case "$(uname -s):$(uname -m)" in
    Linux:x86_64) target=linux-x86_64 ;;
    Linux:aarch64) target=linux-arm64 ;;
    Darwin:arm64) target=macos-arm64 ;;
    Darwin:x86_64) target=macos-x86_64 ;;
    *) printf 'Unsupported DornGuard distribution target\n' >&2; exit 2 ;;
esac

bin_directory="${DORNGUARD_BIN_DIR:-$HOME/.local/bin}"
release_directory="${XDG_DATA_HOME:-$HOME/.local/share}/dornguard/releases"
mkdir -p "$bin_directory" "$release_directory"
release_directory=$(CDPATH='' cd -- "$release_directory" && pwd)
destination="$release_directory/$version"
link="$bin_directory/dornguard"
if [ -e "$link" ] && [ ! -L "$link" ]; then
    printf 'An existing installation is at %s\n' "$link" >&2
    exit 2
fi

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
