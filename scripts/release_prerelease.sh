#!/usr/bin/env bash
set -euo pipefail

VERSION=${1:?usage: release_prerelease.sh VERSION}
VERSION_WITHOUT_BUILD=${VERSION%%+*}
if [[ "$VERSION_WITHOUT_BUILD" == *-* ]]; then
  printf 'true\n'
else
  printf 'false\n'
fi
