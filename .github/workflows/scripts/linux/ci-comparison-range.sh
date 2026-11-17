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
            # Carry pending source changes through test-only retries.
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
        ;;
esac
printf 'base_sha=%s\nhead_sha=%s\nforce_full=%s\n' "$base" "$head" "$force_full" >> "${GITHUB_OUTPUT:?}"
