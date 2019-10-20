#!/bin/sh
set -o errexit
set -o nounset

# message_error
#
# Print an error message and quit.
#
# Args:
#     $1 - The reason for exiting.
message_error()
{
  echo "ERROR: $1"
  exit 1
}

# message_usage
#
# Print usage information.
message_usage()
{
  echo "Usage:"
  echo "  $(basename "$0") {-I --install}   [-g] [-p]"
  echo "  $(basename "$0") {-T --test}      [-g]"
  echo "  $(basename "$0") {-U --uninstall} [-g]"
  echo "  $(basename "$0") {-h --help}"
  echo
  echo "Options:"
  echo "  -p  Install system packages"
  echo "  -g  GUI"
  exit
}

# message_worker
#
# Print a message if a worker did work.
#
# Args:
#     $1 - The message.
message_worker()
{
  if [ "${_work-unset}" = "unset" ] \
    || ! $_work
  then
    _work=true
    echo ":: $1..."
  fi
}