#!/bin/sh
set -o errexit
set -o nounset

FILE="$(basename "$(readlink -f "$0")")"
export FILE

. "$DIR"/src/logger.sh

find "$DIR"/test/tmp -type f -not -name ".gitkeep" -delete

# start

(
  # init

  TEST="log_error"
  export TEST

  # test

  tmp="$(mktemp)"
  code=0
  (log_error "foo") > "$tmp" || code=$?
  output="$(cat "$tmp")"

  # assert

  [ "ERROR: foo" = "$output" ] || log_fail "$LINENO" "log_error output expected 'ERROR: foo' actual '$output'"
  [ 1 -eq $code ]              || log_fail "$LINENO" "log_error exit code expected '1' actual '$code'"
)

(
  # init

  TEST="log_stage"
  export TEST

  # test

  tmp="$(mktemp)"
  code=0
  (log_stage "foo" && log_stage "bar") > "$tmp" || code=$?
  output="$(cat "$tmp")"

  # assert

  [ ":: foo..." = "$output" ]  || log_fail "$LINENO" "log_stage output expected ':: foo...' actual '$output'"
  [ 0 -eq $code ]              || log_fail "$LINENO" "log_stage exit code expected '0' actual '$code'"
)