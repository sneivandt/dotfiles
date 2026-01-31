#!/bin/sh
set -o errexit
set -o nounset

if [ -f .git/info/sparse-checkout ]; then
  if ! grep -q "windows" .git/info/sparse-checkout; then
    echo "Error: Windows files should be excluded but no Windows exclusion patterns found"
    exit 1
  fi
  echo "✓ Sparse checkout excludes Windows"
fi
