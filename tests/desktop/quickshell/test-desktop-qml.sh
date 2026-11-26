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
  if "$linter" --help 2>&1 | grep -q -- --only-explicit-categories; then
    if [ "$mode" = production ]; then
      set -- --read-only-property=error "$@"
    fi
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
    # Qt 6.8 lacks --only-explicit-categories and the `error` severity level;
    # `warning` is its failing level. Disable categories that require compiled
    # Quickshell metatypes while failing on syntax and structural errors.
    if [ "$mode" = production ]; then
      set -- --read-only-property=warning "$@"
    else
      set -- --read-only-property=disable "$@"
    fi
    set -- \
      --ignore-settings \
      --required=disable \
      --unresolved-alias=disable \
      --import=disable \
      --deprecated=disable \
      --signal-handler-parameters=disable \
      --missing-type=disable \
      --unresolved-type=disable \
      --restricted-type=disable \
      --prefixed-import-type=disable \
      --incompatible-type=disable \
      --missing-property=disable \
      --non-list-property=disable \
      --unqualified=disable \
      --unused-imports=disable \
      --multiline-strings=disable \
      --use-proper-function=disable \
      --access-singleton-via-object=disable \
      --top-level-component=disable \
      --uncreatable-type=disable \
      --Quick.layout-positioning=disable \
      --Quick.attached-property-type=disable \
      --Quick.anchor-combinations=disable \
      --Quick.controls-native-customize=disable \
      --Quick.unexpected-var-type=disable \
      --Quick.property-changes-parsed=disable \
      --duplicate-property-binding=warning \
      --duplicated-name=warning \
      --alias-cycle=warning \
      --inheritance-cycle=warning \
      --invalid-lint-directive=warning \
      --var-used-before-declaration=warning \
      --with=warning \
      "$@"
  fi
  "$linter" \
    -I "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
    -I "$REPO_ROOT/symlinks/config/quickshell" \
    "$@"
}

if [ -x "$linter" ] || command -v "$linter" >/dev/null 2>&1; then
  set --
  for file in $(find "$REPO_ROOT/symlinks/config/quickshell" -type f \( -name '*.qml' -o -name '*.js' \) | LC_ALL=C sort); do
    set -- "$@" "$file"
  done
  run_qmllint production "$@"
  run_qmllint test \
    "$REPO_ROOT"/tests/desktop/quickshell/qml/tst_*.qml
fi

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML_DISABLE_DISK_CACHE=1
exec "$runner" -import "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
  -input "$REPO_ROOT/tests/desktop/quickshell/qml"
