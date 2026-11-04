#!/bin/sh
# A selective push must inherit coverage from a successful preceding push.
set -eu

base=
head=
force_full=false
case "${GITHUB_EVENT_NAME:?}" in
    pull_request)
        base=${PR_BASE:?}
        head=${PR_HEAD:?}
        ;;
    push)
        before=${PUSH_BEFORE:-}
        if [ -n "$before" ] && [ "$before" != 0000000000000000000000000000000000000000 ] &&
            git merge-base --is-ancestor "$before" "${GITHUB_SHA:?}"; then
            base=$before
            head=$GITHUB_SHA
        fi
        if [ -z "$base" ] ||
            ! previous=$(gh api --method GET "repos/${GITHUB_REPOSITORY:?}/actions/workflows/ci.yml/runs" \
                -f event=push -f branch=main -f head_sha="$base" -F per_page=1 \
                --jq '.workflow_runs[0] | select(.conclusion == "success") | .head_sha') ||
            [ "$previous" != "$base" ]; then
            echo "Preceding push CI could not be verified. Selecting full CI."
            force_full=true
        fi
        ;;
    workflow_dispatch)
        force_full=true
        if [ -n "${MUTATION_BASE:-}" ]; then
            case "$MUTATION_BASE" in
                *[!0-9a-f]*) echo "Mutation base must be a full commit SHA." >&2; exit 1 ;;
            esac
            if [ "${#MUTATION_BASE}" != 40 ] ||
                ! git merge-base --is-ancestor "$MUTATION_BASE" "${GITHUB_SHA:?}"; then
                echo "Mutation base must be a full SHA reachable from the selected commit." >&2
                exit 1
            fi
            base=$MUTATION_BASE
            head=$GITHUB_SHA
        fi
        ;;
esac
printf 'base_sha=%s\nhead_sha=%s\nforce_full=%s\n' "$base" "$head" "$force_full" >> "${GITHUB_OUTPUT:?}"
