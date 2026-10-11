#!/bin/sh
set -eu

REPO_ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)
# Embedded shell lint has separate coverage; check workflow syntax, expressions,
# action inputs and contexts without depending on optional embedded linters.
exec actionlint -shellcheck= -pyflakes= "$REPO_ROOT"/.github/workflows/*.yml
