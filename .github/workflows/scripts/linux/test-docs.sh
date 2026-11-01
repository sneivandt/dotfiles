#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-docs.sh — Documentation consistency CI tests.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Python preserves Git's NUL-delimited filenames and Markdown destinations.
docs_links()
{
  log_stage "Checking documentation links and repository skill metadata"
  _skill_checks="$DIR/.agents/skills/skill-maintenance/scripts"
  python3 -B "$_skill_checks/test-check-skills.py" || return 1
  python3 -B "$_skill_checks/check-skills.py" "$DIR" || return 1
  python3 -B "$SCRIPT_DIR/check-doc-links.py" "$DIR"
}

# Compare documentation to the real public catalog in an isolated Rust fixture.
docs_task_selectors()
{
  log_stage "Checking documented task selectors"
  (cd "$DIR/cli" && cargo test --locked --quiet --profile ci --lib \
    app::commands::list::tests::documented_selectors_match_catalog -- --exact)
}

docs_site()
{
  log_stage "Checking the built documentation and browser behavior"
  (
    cd "$DIR/docs"
    export ASTRO_TELEMETRY_DISABLED=1
    npm run check &&
      npm test &&
      npm run build &&
      python3 -B "$SCRIPT_DIR/check-doc-site.py" "$DIR" &&
      npm run test:browser
  )
}

# Execute one or more tests when run directly: sh test-docs.sh <function_name>...
case "$0" in
  *test-docs.sh)
    for test_name in "$@"; do
      "$test_name"
    done
    ;;
esac
