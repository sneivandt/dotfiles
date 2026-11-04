#!/bin/sh
# Run only in the disposable Arch CI container.
set -eu
test -e /.dockerenv
test "$(id -u)" = 0
test -e /etc/arch-release

pacman -Syu --noconfirm --needed git sudo libgit2
useradd -m smoke
fixture=/home/smoke/fixture
install -d -o smoke -g smoke "$fixture" "$fixture/conf" "$fixture/system"
for name in symlinks chmod systemd-units vscode-extensions git-config agent-settings registry; do
  touch "$fixture/conf/$name.toml"
done
printf '[arch]\npackages = ["tree"]\n' > "$fixture/conf/packages.toml"
cat > "$fixture/conf/system-files.toml" <<'TOML'
[arch]
files = [{ source = "fixture.toml", target = "/etc/dotfiles-ci/config.toml", merge = "toml" }]
TOML
printf 'managed = true\n' > "$fixture/system/fixture.toml"
install -d /etc/dotfiles-ci
printf 'local_setting = 42\n' > /etc/dotfiles-ci/config.toml
cp /etc/dotfiles-ci/config.toml /tmp/config.before
install -m 0755 /workspace/bin/dotfiles /usr/local/bin/dotfiles
dotfiles --version
runuser -u smoke -- git init -q "$fixture"

run_update() {
  runuser -u smoke -- env HOME=/home/smoke XDG_STATE_HOME=/home/smoke/state \
    DOTFILES_LOG_DIR=/home/smoke/logs DOTFILES_SKIP_SELF_UPDATE=1 \
    dotfiles update --root "$fixture" --profile base --non-interactive \
      --fail-on-skip --no-repo-update --only "$1"
}

if run_update system-files; then
  echo "system-files unexpectedly succeeded without sudo access" >&2
  exit 1
fi
cmp /tmp/config.before /etc/dotfiles-ci/config.toml

printf 'smoke ALL=(root) NOPASSWD: ALL\n' >/etc/sudoers.d/dotfiles-ci
chmod 0440 /etc/sudoers.d/dotfiles-ci
if pacman -Q tree >/dev/null 2>&1; then
  echo "the fixture package is already installed" >&2
  exit 1
fi
run_update packages,system-files
pacman -Q tree
grep -Fx 'local_setting = 42' /etc/dotfiles-ci/config.toml
grep -Fx 'managed = true' /etc/dotfiles-ci/config.toml
state=$(stat -c '%i:%Y' /etc/dotfiles-ci/config.toml)
run_update packages,system-files
test "$state" = "$(stat -c '%i:%Y' /etc/dotfiles-ci/config.toml)"
