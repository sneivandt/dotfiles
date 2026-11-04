#!/bin/sh
# A selective push must inherit coverage from a successful preceding push.
set -eu

base=
head=
case "${GITHUB_EVENT_NAME:?}" in
    pull_request)
        base=${PR_BASE:?}
        head=${PR_HEAD:?}
        ;;
    push)
        before=${PUSH_BEFORE:-}
        if [ -n "$before" ] && [ "$before" != 0000000000000000000000000000000000000000 ] &&
            git merge-base --is-ancestor "$before" "${GITHUB_SHA:?}" &&
            previous=$(gh api --method GET "repos/${GITHUB_REPOSITORY:?}/actions/workflows/ci.yml/runs" \
                -f event=push -f branch=main -f head_sha="$before" -F per_page=1 \
                --jq '.workflow_runs[0] | select(.conclusion == "success") | .head_sha') &&
            [ "$previous" = "$before" ]; then
            base=$before
            head=$GITHUB_SHA
        else
            echo "Preceding push CI could not be verified. Selecting full CI."
        fi
        ;;
esac
printf 'base_sha=%s\nhead_sha=%s\n' "$base" "$head" >> "${GITHUB_OUTPUT:?}"
