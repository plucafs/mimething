#!/usr/bin/env bash
# Release mimething: bump version, commit, tag, push — CI does the rest.
# Usage: ./release.sh 1.0.1
set -euo pipefail

if [ $# -ne 1 ] || ! [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Usage: $0 <version>   (semver, e.g. 1.0.1)" >&2
    exit 1
fi

VERSION="$1"

if [ -n "$(git status --porcelain)" ]; then
    echo "Working tree not clean — commit or stash first." >&2
    git status --short >&2
    exit 1
fi

cargo set-version "$VERSION"
git add Cargo.toml Cargo.lock
git commit -m "chore: release $VERSION"
git push origin main
git tag "v$VERSION"
git push origin "v$VERSION"

echo "Released v$VERSION — watch CI with: gh run watch --repo plucafs/mimething"
