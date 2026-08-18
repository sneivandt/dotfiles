#!/bin/sh
set -o errexit
set -o nounset

if [ "$#" -eq 0 ]; then
  printf "ERROR: at least one APT package is required\n" >&2
  exit 2
fi

# GitHub's hosted Ubuntu runners prefer an Azure mirror that can occasionally
# accept a connection without returning package indexes. Use the canonical
# archive directly so the request-level timeouts below can retry another server.
sources=/etc/apt/sources.list.d/ubuntu.sources
mirror='mirror+file:/etc/apt/apt-mirrors.txt'
if [ "${GITHUB_ACTIONS:-}" = "true" ] &&
   [ -f "$sources" ] &&
   grep -Fq "$mirror" "$sources"
then
  sudo sed -i "s|$mirror|https://archive.ubuntu.com/ubuntu|g" "$sources"
fi

run_apt()
{
  sudo timeout --kill-after=30s 5m apt-get \
    -o Acquire::Retries=3 \
    -o Acquire::http::Timeout=30 \
    -o Acquire::https::Timeout=30 \
    "$@"
}

run_apt update
run_apt install -y "$@"
