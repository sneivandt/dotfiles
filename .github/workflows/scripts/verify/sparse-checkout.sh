#!/bin/sh
set -o errexit
set -o nounset

if [ -f .git/info/sparse-checkout ]; then
  echo "Sparse checkout configured:"
  cat .git/info/sparse-checkout
else
  echo "Error: sparse checkout file not found"
  exit 1
fi
