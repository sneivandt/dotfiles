#!/bin/sh
set -eu

: "${DIR:?DIR must be set}"
: "${HEAD_SHA:?HEAD_SHA must be set}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT must be set}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"

cd "$DIR"
git rev-parse --verify "$HEAD_SHA^{commit}" >/dev/null
# Compare with a published release, not the previous push: unsuccessful or
# superseded publishing runs must not lose pending binary changes.
tag=$(gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts \
  --exclude-pre-releases --limit 1 --json tagName --jq '.[0].tagName // empty')
if [ -z "$tag" ]; then
  echo "No published release exists; creating the initial release."
  printf 'run_release=true\n' >> "$GITHUB_OUTPUT"
  exit 0
fi

base=$(git rev-parse --verify "refs/tags/$tag^{commit}")
if git merge-base --is-ancestor "$HEAD_SHA" "$base"; then
  echo "This commit is already covered by a published release."
  printf 'run_release=false\n' >> "$GITHUB_OUTPUT"
  exit 0
fi
if ! git merge-base --is-ancestor "$base" "$HEAD_SHA"; then
  echo "Published release $tag is not an ancestor of the tested commit; refusing to publish." >&2
  exit 1
fi

echo "Comparing binary inputs with published release $tag."
BASE_SHA="$base" GITHUB_EVENT_NAME=release \
  sh .github/workflows/scripts/linux/classify-ci-changes.sh
