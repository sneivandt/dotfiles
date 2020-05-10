#!/bin/sh
set -o errexit
set -o nounset

# log_error
#
# Log an error message and quit.
#
# Args:
#     $1 - The reason for exiting.
log_error()
{
  echo "ERROR: $1"
  exit 1
}

# log_fail
#
# Log a test failure.
#
# Args:
#     $1 - Line number.
#     $2 - Message.
log_fail()
{
  echo "FAIL $FILE $TEST $1 : $2"
}

# log_usage
#
# Log usage information.
log_usage()
{
  echo "Usage:"
  echo "  $(basename "$0")"
  echo "  $(basename "$0") {-I --install}   [-g] [-p]"
  echo "  $(basename "$0") {-U --uninstall} [-g]"
  echo "  $(basename "$0") {-T --test}"
  echo "  $(basename "$0") {-h --help}"
  echo
  echo "Options:"
  echo "  -g  Configure GUI environment"
  echo "  -p  Install system packages"
  exit
}

# log_stage
#
# Log a message if a stage did work.
#
# Args:
#     $1 - The message.
log_stage()
{
  if [ "${_work-unset}" = "unset" ] \
    || ! $_work
  then
    _work=true
    echo ":: $1..."
  fi
}