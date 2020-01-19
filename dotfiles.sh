#!/bin/sh
set -o errexit
set -o nounset

DIR="$(dirname "$(readlink -f "$0")")"
export DIR

. "$DIR"/src/logger.sh

if [ "$(id -u)" = 0 ]
then
  log_error "$(basename "$0") can not be run as root."
fi

. "$DIR"/src/workflows.sh

case ${1:-} in
  -I* | --install)
    OPT="$(getopt -o Ipg -l install -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_install
    ;;
  -T* | --test)
    OPT="$(getopt -o Tg -l test -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_test
    ;;
  -U* | --uninstall)
    OPT="$(getopt -o Ug -l uninstall -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    do_uninstall
    ;;
  -h | --help)
    OPT="$(getopt -o h -l help -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    message_usage
    ;;
  *)
    OPT="$(getopt -o -l -n "$(basename "$0")" -- "$@")" \
      || exit 1
    export OPT
    message_usage
    ;;
esac