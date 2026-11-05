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
        head=${GITHUB_SHA:?}
        workflow_runs="repos/${GITHUB_REPOSITORY:?}/actions/workflows/ci.yml/runs"
        if [ -n "$before" ] && git merge-base --is-ancestor "$before" "$head" &&
            [ "$(gh api --method GET "$workflow_runs" -f event=push -f branch=main \
                -f head_sha="$before" -F per_page=1 \
                --jq '.workflow_runs[0] | select(.conclusion == "success") | .head_sha')" = "$before" ]; then
            base=$before
        else
            echo "Preceding push CI could not be verified. Selecting full CI."
            force_full=true
            # A test-only retry must still exercise all source changes that
            # have not passed CI. Filter out unrelated history and this run.
            if ! candidates=$(gh api --method GET "$workflow_runs" -f event=push \
                -f branch=main -f status=success -F per_page=100 \
                --jq '.workflow_runs[].head_sha'); then
                echo "Cannot verify a successful CI comparison base." >&2
                exit 1
            fi
            for candidate in $candidates; do
                if [ "$candidate" != "$head" ] && git merge-base --is-ancestor "$candidate" "$head"; then
                    base=$candidate
                    break
                fi
            done
            if [ -z "$base" ]; then
                # Initial CI has no prior coverage to inherit.
                base=$(git hash-object -t tree /dev/null)
            fi
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
