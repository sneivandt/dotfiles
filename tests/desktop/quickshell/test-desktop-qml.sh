#!/bin/sh
set -eu

REPO_ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../../.." && pwd)
if [ -n "${QT_ROOT_DIR:-}" ]; then
  runner="$QT_ROOT_DIR/bin/qmltestrunner"
  linter="$QT_ROOT_DIR/bin/qmllint"
elif [ -x /usr/lib/qt6/bin/qmltestrunner ]; then
  runner=/usr/lib/qt6/bin/qmltestrunner
  linter=/usr/lib/qt6/bin/qmllint
else
  runner=qmltestrunner
  linter=qmllint
fi

run_qmllint() {
  mode=$1
  shift
  if [ "$mode" = production ]; then
    set -- --read-only-property=error "$@"
  fi
  if "$linter" --help 2>&1 | grep -q -- --only-explicit-categories; then
    set -- \
      --only-explicit-categories \
      --syntax=error \
      --duplicate-property-binding=error \
      --duplicated-name=error \
      --duplicate-import=error \
      --alias-cycle=error \
      --inheritance-cycle=error \
      --invalid-lint-directive=error \
      --assignment-in-condition=error \
      --unintentional-empty-block=error \
      --unreachable-code=error \
      --var-used-before-declaration=error \
      --eval=error \
      --with=error \
      "$@"
  else
    # Qt 6.8 lacks --only-explicit-categories and several 6.9+ categories.
    # -W -1 ignores unresolvable Quickshell metatype warnings while still
    # failing on syntax errors and every explicit =error category.
    set -- \
      -W -1 \
      --ignore-settings \
      --import=disable \
      --missing-property=disable \
      --missing-type=disable \
      --unqualified=disable \
      --incompatible-type=disable \
      --signal-handler-parameters=disable \
      --unused-imports=disable \
      --duplicate-property-binding=error \
      --duplicated-name=error \
      --alias-cycle=error \
      --inheritance-cycle=error \
      --invalid-lint-directive=error \
      --var-used-before-declaration=error \
      --with=error \
      "$@"
  fi
  "$linter" \
    -I "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
    -I "$REPO_ROOT/symlinks/config/quickshell" \
    "$@"
}

if [ -x "$linter" ] || command -v "$linter" >/dev/null 2>&1; then
  run_qmllint production \
    "$REPO_ROOT"/symlinks/config/quickshell/*.qml \
    "$REPO_ROOT"/symlinks/config/quickshell/*.js
  run_qmllint test \
    "$REPO_ROOT"/tests/desktop/quickshell/qml/tst_*.qml
fi

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML_DISABLE_DISK_CACHE=1
exec "$runner" -import "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
  -input "$REPO_ROOT/tests/desktop/quickshell/qml"
