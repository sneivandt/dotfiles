#!/bin/sh
set -o errexit
set -o nounset

FILE="$(basename "$(readlink -f "$0")")"
export FILE

. "$DIR"/src/logger.sh

find "$DIR"/test/tmp -type f -not -name ".gitkeep" -delete

# start

. "$DIR"/src/utils.sh

(
  # init

  TEST="is_flag_set"
  export TEST

  # test

  OPT=""
  export OPT

  # assert

  ! is_flag_set ""    || log_fail "$LINENO" "'' should not be set"
  ! is_flag_set " "   || log_fail "$LINENO" "' ' should not be set"
  ! is_flag_set "a"   || log_fail "$LINENO" "'a' should not be set"
  ! is_flag_set "a\n" || log_fail "$LINENO" "'a\n' should not be set"
  ! is_flag_set " a"  || log_fail "$LINENO" "' a' should not be set"
  ! is_flag_set "a "  || log_fail "$LINENO" "'a ' should not be set"
  ! is_flag_set "b"   || log_fail "$LINENO" "'b' should not be set"
  ! is_flag_set "-a"  || log_fail "$LINENO" "'-a' should not be set"
  ! is_flag_set "aa"  || log_fail "$LINENO" "'aa' should not be set"
  ! is_flag_set "A"   || log_fail "$LINENO" "'A' should not be set"

  # test

  OPT="-a"
  export OPT

  # assert

  ! is_flag_set ""    || log_fail "$LINENO" "'' should not be set"
  ! is_flag_set " "   || log_fail "$LINENO" "' ' should not be set"
    is_flag_set "a"   || log_fail "$LINENO" "'a' should be set"
  ! is_flag_set "a\n" || log_fail "$LINENO" "'a\n' should not be set"
  ! is_flag_set " a"  || log_fail "$LINENO" "' a' should not be set"
  ! is_flag_set "a "  || log_fail "$LINENO" "'a ' should not be set"
  ! is_flag_set "b"   || log_fail "$LINENO" "'b' should not be set"
  ! is_flag_set "-a"  || log_fail "$LINENO" "'-a' should not be set"
  ! is_flag_set "aa"  || log_fail "$LINENO" "'aa' should not be set"
  ! is_flag_set "A"   || log_fail "$LINENO" "'A' should not be set"

  # test

  OPT="-b"
  export OPT

  # assert

  ! is_flag_set ""    || log_fail "$LINENO" "'' should not be set"
  ! is_flag_set " "   || log_fail "$LINENO" "' ' should not be set"
  ! is_flag_set "a"   || log_fail "$LINENO" "'a' should not be set"
  ! is_flag_set "a\n" || log_fail "$LINENO" "'a\n' should not be set"
  ! is_flag_set " a"  || log_fail "$LINENO" "' a' should not be set"
  ! is_flag_set "a "  || log_fail "$LINENO" "'a ' should not be set"
    is_flag_set "b"   || log_fail "$LINENO" "'b' should be set"
  ! is_flag_set "-a"  || log_fail "$LINENO" "'-a' should not be set"
  ! is_flag_set "aa"  || log_fail "$LINENO" "'aa' should not be set"
  ! is_flag_set "A"   || log_fail "$LINENO" "'A' should not be set"

  # test

  OPT="-a -b"
  export OPT

  # assert

  ! is_flag_set ""    || log_fail "$LINENO" "'' should not be set"
  ! is_flag_set " "   || log_fail "$LINENO" "' ' should not be set"
    is_flag_set "a"   || log_fail "$LINENO" "'a' should be set"
  ! is_flag_set "a\n" || log_fail "$LINENO" "'a\n' should not be set"
  ! is_flag_set " a"  || log_fail "$LINENO" "' a' should not be set"
  ! is_flag_set "a "  || log_fail "$LINENO" "'a ' should not be set"
    is_flag_set "b"   || log_fail "$LINENO" "'b' should be set"
  ! is_flag_set "-a"  || log_fail "$LINENO" "'-a' should not be set"
  ! is_flag_set "aa"  || log_fail "$LINENO" "'aa' should not be set"
  ! is_flag_set "A"   || log_fail "$LINENO" "'A' should not be set"

  # test

  OPT="-apple"
  export OPT

  # assert

  ! is_flag_set ""    || log_fail "$LINENO" "'' should not be set"
  ! is_flag_set " "   || log_fail "$LINENO" "' ' should not be set"
  ! is_flag_set "a"   || log_fail "$LINENO" "'a' should not be set"
  ! is_flag_set "a\n" || log_fail "$LINENO" "'a\n' should not be set"
  ! is_flag_set "a"   || log_fail "$LINENO" "'a' should not be set"
  ! is_flag_set " a"  || log_fail "$LINENO" "' a' should not be set"
  ! is_flag_set "a "  || log_fail "$LINENO" "'a ' should not be set"
  ! is_flag_set "-a"  || log_fail "$LINENO" "'-a' should not be set"
  ! is_flag_set "aa"  || log_fail "$LINENO" "'aa' should not be set"
  ! is_flag_set "A"   || log_fail "$LINENO" "'A' should not be set"

  # cleanup

  OPT=""
  export OPT
)

(
  # init

  TEST="is_env_ignored"
  export TEST

  # mock

  printf "ID=foo" > "$DIR"/test/tmp/cat

  # test

  OPT=""
  export OPT

  # assert

  ! is_env_ignored ""         || log_fail "$LINENO" "'' should not be ignored"
  ! is_env_ignored "foo"      || log_fail "$LINENO" "'foo' should not be ignored"
  ! is_env_ignored "base"     || log_fail "$LINENO" "'base' should not be ignored"
    is_env_ignored "base-gui" || log_fail "$LINENO" "'base-gui' should be ignored"
    is_env_ignored "arch"     || log_fail "$LINENO" "'arch' should be ignored"
    is_env_ignored "arch-gui" || log_fail "$LINENO" "'arch-gui' should be ignored"
  ! is_env_ignored "arch "    || log_fail "$LINENO" "'arch ' should not be ignored"
  ! is_env_ignored "Arch"     || log_fail "$LINENO" "'Arch' should not be ignored"

  # mock

  printf "ID=foo" > "$DIR"/test/tmp/cat

  # test

  OPT="-g"
  export OPT

  # assert

  ! is_env_ignored ""         || log_fail "$LINENO" "'' should not be ignored"
  ! is_env_ignored "foo"      || log_fail "$LINENO" "'foo' should not be ignored"
  ! is_env_ignored "base"     || log_fail "$LINENO" "'base' should not be ignored"
  ! is_env_ignored "base-gui" || log_fail "$LINENO" "'base-gui' should not be ignored"
    is_env_ignored "arch"     || log_fail "$LINENO" "'arch' should be ignored"
    is_env_ignored "arch-gui" || log_fail "$LINENO" "'arch-gui' should be ignored"
  ! is_env_ignored "arch "    || log_fail "$LINENO" "'arch ' should not be ignored"
  ! is_env_ignored "Arch"     || log_fail "$LINENO" "'Arch' should not be ignored"

  # mock

  printf "ID=arch" > "$DIR"/test/tmp/cat

  # test

  OPT=""
  export OPT

  # assert

  ! is_env_ignored ""         || log_fail "$LINENO" "'' should not be ignored"
  ! is_env_ignored "foo"      || log_fail "$LINENO" "'foo' should not be ignored"
  ! is_env_ignored "base"     || log_fail "$LINENO" "'base' should not be ignored"
    is_env_ignored "base-gui" || log_fail "$LINENO" "'base-gui' should be ignored"
  ! is_env_ignored "arch"     || log_fail "$LINENO" "'arch' should not be ignored"
    is_env_ignored "arch-gui" || log_fail "$LINENO" "'arch-gui' should be ignored"
  ! is_env_ignored "arch "    || log_fail "$LINENO" "'arch ' should not be ignored"
  ! is_env_ignored "Arch"     || log_fail "$LINENO" "'Arch' should not be ignored"

  # mock

  printf "ID=arch" > "$DIR"/test/tmp/cat

  # test

  OPT="-g"
  export OPT

  # assert

  ! is_env_ignored ""         || log_fail "$LINENO" "'' should not be ignored"
  ! is_env_ignored "foo"      || log_fail "$LINENO" "'foo' should not be ignored"
  ! is_env_ignored "base"     || log_fail "$LINENO" "'base' should not be ignored"
  ! is_env_ignored "base-gui" || log_fail "$LINENO" "'base-gui' should not be ignored"
  ! is_env_ignored "arch"     || log_fail "$LINENO" "'arch' should not be ignored"
  ! is_env_ignored "arch-gui" || log_fail "$LINENO" "'arch-gui' should not be ignored"
  ! is_env_ignored "arch "    || log_fail "$LINENO" "'arch ' should not be ignored"
  ! is_env_ignored "Arch"     || log_fail "$LINENO" "'Arch' should not be ignored"

  # clean up

  OPT=""
  export OPT
  printf "" > "$DIR"/test/tmp/cat
)

(
  # init

  TEST="is_shell_script"
  export TEST

  # assert

  ! is_shell_script ""                 || log_fail "$LINENO" "'' is not shell script"
  ! is_shell_script "/"                || log_fail "$LINENO" "'/' is not shell script"
  ! is_shell_script "/invalid"         || log_fail "$LINENO" "'/invalid' is not shell script"
  ! is_shell_script "$DIR"             || log_fail "$LINENO" "'$DIR' is not shell script"
    is_shell_script "$DIR/dotfiles.sh" || log_fail "$LINENO" "'$DIR/dotfiles.sh' is a shell script"
  ! is_shell_script "$DIR/README.md"   || log_fail "$LINENO" "'$DIR/README.md' is not shell script"
)