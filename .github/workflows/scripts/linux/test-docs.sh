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

# List every tracked markdown file, one path relative to $DIR per line.
list_markdown_files() {
  if git -C "$DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$DIR" ls-files '*.md'
  else
    ( cd "$DIR" && find . -type f -name '*.md' -not -path './.git/*' | sed 's|^\./||' )
  fi
}

# Print the link destinations of one markdown file, one per line.
#
# Only inline links and images are considered; reference definitions, bare URLs,
# and autolinks are out of scope.
extract_link_targets() {
  grep -oE '\]\([^)]+\)' "$1" 2>/dev/null | sed 's/^](//;s/)$//' || true
}

# Verify every relative markdown link resolves to a file in the repository.
#
# External links, mailto targets, and same-document anchors are skipped; a link
# with an anchor is checked for the file part only.
docs_links()
{(
  log_stage "Checking documentation links"
  failures=0
  checked=0

  for doc in $(list_markdown_files); do
    doc_dir="$(dirname "$DIR/$doc")"
    for target in $(extract_link_targets "$DIR/$doc"); do
      case "$target" in
        http://*|https://*|mailto:*|'#'*|'<'*) continue ;;
      esac

      # Drop any anchor or query suffix; only the path is resolvable.
      path="${target%%#*}"
      path="${path%%\?*}"
      [ -n "$path" ] || continue

      case "$path" in
        /*) resolved="$DIR$path" ;;
        *) resolved="$doc_dir/$path" ;;
      esac

      checked=$((checked + 1))
      if [ ! -e "$resolved" ]; then
        printf "   %s: broken link -> %s\n" "$doc" "$target"
        failures=$((failures + 1))
      fi
    done
  done

  log_verbose "Checked $checked relative links"
  [ "$failures" -eq 0 ] || log_error "$failures broken documentation link(s)"
)}

# Verify every task selector documented in docs/TASKS.md still exists in the CLI.
#
# Selectors are declared either through the task macros (`selector: "name"`) or
# by overriding `fn selector`, so both forms are collected.
docs_task_selectors()
{(
  tasks_doc="$DIR/docs/TASKS.md"
  if [ ! -f "$tasks_doc" ] || [ ! -d "$DIR/cli/src" ]; then
    log_verbose "Skipping task selector check: docs/TASKS.md or cli/src missing"
    return 0
  fi

  log_stage "Checking documented task selectors"

  code_selectors="$(mktemp)"
  documented_selectors="$(mktemp)"
  trap 'rm -f "$code_selectors" "$documented_selectors"' EXIT HUP INT TERM

  # Selectors are declared three ways: the `task_metadata!` macro field, a
  # `TaskMeta::new(..).with_selector("..")` builder call, and (legacy) a
  # hand-written `fn selector` override.
  {
    grep -rhoE 'selector: "[a-z0-9-]+"' "$DIR/cli/src" | sed 's/.*"\(.*\)"/\1/'
    grep -rhoE 'with_selector\("[a-z0-9-]+"\)' "$DIR/cli/src" --include='*.rs' \
      | sed 's/.*"\(.*\)".*/\1/'
    grep -rhA2 'fn selector' "$DIR/cli/src" --include='*.rs' \
      | grep -oE '^[[:space:]]+"[a-z0-9-]+"' | tr -d ' "'
  } | sort -u > "$code_selectors"

  # Selector column of the catalog tables: rows beginning with | `selector` |
  grep -oE '^\| `[a-z0-9-]+`' "$tasks_doc" | tr -d '|` ' | sort -u > "$documented_selectors"

  failures=0
  while IFS= read -r selector || [ -n "$selector" ]; do
    if ! grep -qx "$selector" "$code_selectors"; then
      printf "   docs/TASKS.md documents unknown selector: %s\n" "$selector"
      failures=$((failures + 1))
    fi
  done < "$documented_selectors"

  log_verbose "Checked $(wc -l < "$documented_selectors" | tr -d ' ') documented selectors"
  [ "$failures" -eq 0 ] || log_error "$failures documented selector(s) no longer exist"
)}

# Execute one or more tests when run directly: sh test-docs.sh <function_name>...
case "$0" in
  *test-docs.sh)
    for test_name in "$@"; do
      "$test_name"
    done
    ;;
esac
