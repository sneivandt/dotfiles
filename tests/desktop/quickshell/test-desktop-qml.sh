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

if [ -x "$linter" ] || command -v "$linter" >/dev/null 2>&1; then
  "$linter" --only-explicit-categories \
    --syntax=error \
    --duplicate-property-binding=error \
    --duplicated-name=error \
    --duplicate-import=error \
    --alias-cycle=error \
    --inheritance-cycle=error \
    --invalid-lint-directive=error \
    --read-only-property=error \
    --assignment-in-condition=error \
    --unintentional-empty-block=error \
    --unreachable-code=error \
    --var-used-before-declaration=error \
    --eval=error \
    --with=error \
    -I "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
    -I "$REPO_ROOT/symlinks/config/quickshell" \
    "$REPO_ROOT"/symlinks/config/quickshell/*.qml \
    "$REPO_ROOT"/symlinks/config/quickshell/*.js
  "$linter" --only-explicit-categories \
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
    -I "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
    -I "$REPO_ROOT/symlinks/config/quickshell" \
    "$REPO_ROOT"/tests/desktop/quickshell/qml/tst_*.qml
fi

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML_DISABLE_DISK_CACHE=1
exec "$runner" -import "$REPO_ROOT/tests/desktop/quickshell/qml/mocks" \
  -input "$REPO_ROOT/tests/desktop/quickshell/qml"
