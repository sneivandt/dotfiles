#!/bin/sh
# Give CI profiles and application tests a vacant home instead of runner defaults.
set -eu

temp=${RUNNER_TEMP:?}
if [ "${RUNNER_OS:?}" = Windows ]; then
    temp=$(cygpath -u "$temp")
fi
home=$(mktemp -d "$temp/dotfiles-home.XXXXXXXXXX")
config=$home/.config
if [ "$RUNNER_OS" = Windows ]; then
    home=$(cygpath -w "$home")
    config=$(cygpath -w "$config")
    printf 'USERPROFILE=%s\n' "$home" >> "${GITHUB_ENV:?}"
fi
printf 'HOME=%s\nXDG_CONFIG_HOME=%s\n' "$home" "$config" >> "${GITHUB_ENV:?}"
