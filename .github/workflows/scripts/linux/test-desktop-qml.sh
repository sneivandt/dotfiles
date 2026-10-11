#!/bin/sh
set -eu

REPO_ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)
if [ -n "${QT_ROOT_DIR:-}" ]; then
  runner="$QT_ROOT_DIR/bin/qmltestrunner"
elif [ -x /usr/lib/qt6/bin/qmltestrunner ]; then
  runner=/usr/lib/qt6/bin/qmltestrunner
else
  runner=qmltestrunner
fi

export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML_DISABLE_DISK_CACHE=1
exec "$runner" -import "$REPO_ROOT/symlinks/config/quickshell/tests/qml/mocks" \
  -input "$REPO_ROOT/symlinks/config/quickshell/tests/qml"
