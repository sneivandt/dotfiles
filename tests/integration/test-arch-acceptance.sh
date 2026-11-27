#!/bin/sh
# Run only in the disposable Arch CI container.
set -eu
test -e /.dockerenv
test "$(id -u)" = 0
test -e /etc/arch-release

pacman -Syu --noconfirm --needed git sudo libgit2 systemd pacman-contrib
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

# Exercise the real CLI's offline user-unit path without a service manager.
unit_dir=/home/smoke/.config/systemd/user
install -d -o smoke -g smoke "$unit_dir/audit.service.d"
cat > "$unit_dir/audit.service" <<'UNIT'
[Service]
ExecStart=/usr/bin/true
[Install]
WantedBy=default.target
UNIT
cat > "$unit_dir/audit.service.d/override.conf" <<'UNIT'
[Install]
WantedBy=
WantedBy=other.target
RequiredBy=another.target
UNIT
chown -R smoke:smoke /home/smoke/.config
printf '[base]\nunits = ["audit.service"]\n' > "$fixture/conf/systemd-units.toml"
run_offline_systemd() {
  runuser -u smoke -- env -u CI HOME=/home/smoke XDG_STATE_HOME=/home/smoke/state \
    DOTFILES_LOG_DIR=/home/smoke/logs DOTFILES_SKIP_SELF_UPDATE=1 \
    DOTFILES_PROVISIONING=arch-chroot \
    dotfiles update --root "$fixture" --profile base --non-interactive \
      --fail-on-skip --no-repo-update --only systemd "$@"
}
run_offline_systemd --dry-run
test ! -e "$unit_dir/other.target.wants"
run_offline_systemd
test -L "$unit_dir/other.target.wants/audit.service"
test -L "$unit_dir/another.target.requires/audit.service"
test ! -e "$unit_dir/default.target.wants"
link_state=$(stat -c '%i:%Y' "$unit_dir/other.target.wants/audit.service")
run_offline_systemd
test "$link_state" = "$(stat -c '%i:%Y' "$unit_dir/other.target.wants/audit.service")"
printf '[base]\nunits = [{ name = "audit.service", scope = "user", enabled = false }]\n' > "$fixture/conf/systemd-units.toml"
run_offline_systemd
test ! -L "$unit_dir/other.target.wants/audit.service"
test ! -L "$unit_dir/another.target.requires/audit.service"
run_offline_systemd

# Replaced enablement directories must block preview and apply without writes.
printf '[base]\nunits = ["audit.service"]\n' > "$fixture/conf/systemd-units.toml"
rmdir "$unit_dir/other.target.wants"
install -d -o smoke -g smoke /home/smoke/unmanaged
runuser -u smoke -- ln -s /home/smoke/unmanaged "$unit_dir/other.target.wants"
if run_offline_systemd --dry-run || run_offline_systemd; then
  echo "redirected systemd enablement unexpectedly succeeded" >&2
  exit 1
fi
test -z "$(find /home/smoke/unmanaged -mindepth 1 -print)"

# A failed native package transaction must leave the existing system file intact
# and converge after the package declaration is corrected.
printf '[arch]\npackages = ["dotfiles-ci-package-that-does-not-exist"]\n' > "$fixture/conf/packages.toml"
if run_update packages; then
  echo "an unavailable package unexpectedly converged" >&2
  exit 1
fi
test "$state" = "$(stat -c '%i:%Y' /etc/dotfiles-ci/config.toml)"
printf '[arch]\npackages = ["tree"]\n' > "$fixture/conf/packages.toml"
run_update packages,system-files

# Fail privileged publication after discovery. Atomic installation must preserve
# the administrator file, remove staging, and allow the same update to retry.
cp /etc/dotfiles-ci/config.toml /tmp/config.converged
printf 'managed = true\nretry = true\n' > "$fixture/system/fixture.toml"
install -d -m 0755 /home/smoke/fixture-bin
cat > /home/smoke/fixture-bin/sudo <<'SH'
#!/bin/sh
case "$*" in
  *dotfiles-system-file*)
    exec /usr/bin/sudo env PATH=/home/smoke/fixture-bin:/usr/bin "$@" ;;
esac
exec /usr/bin/sudo "$@"
SH
chmod 0755 /home/smoke/fixture-bin/sudo
cat > /home/smoke/fixture-bin/mv <<'SH'
#!/bin/sh
echo "injected publication interruption" >&2
exit 1
SH
chmod 0755 /home/smoke/fixture-bin/mv
if runuser -u smoke -- env HOME=/home/smoke XDG_STATE_HOME=/home/smoke/state \
  DOTFILES_LOG_DIR=/home/smoke/logs DOTFILES_SKIP_SELF_UPDATE=1 \
  PATH=/home/smoke/fixture-bin:/usr/local/bin:/usr/bin \
  dotfiles update --root "$fixture" --profile base --non-interactive \
    --fail-on-skip --no-repo-update --only system-files; then
  echo "injected publication failure unexpectedly succeeded" >&2
  exit 1
fi
cmp /tmp/config.converged /etc/dotfiles-ci/config.toml
test -z "$(find /etc/dotfiles-ci -maxdepth 1 -name '.dotfiles-system-file.*' -print)"
run_update system-files
grep -Fx 'retry = true' /etc/dotfiles-ci/config.toml
state=$(stat -c '%i:%Y' /etc/dotfiles-ci/config.toml)
run_update system-files
test "$state" = "$(stat -c '%i:%Y' /etc/dotfiles-ci/config.toml)"

# Validate tracked Arch system fragments against real Arch tooling.
pacman_out=$(pacman-conf -c /workspace/system/pacman.conf)
printf '%s\n' "$pacman_out" | grep -Fx 'Color'
printf '%s\n' "$pacman_out" | grep -Fx 'ILoveCandy'
printf '%s\n' "$pacman_out" | grep -Fx 'VerbosePkgLists'
printf '%s\n' "$pacman_out" | grep -Fx 'ParallelDownloads = 5'

install -d /etc/systemd/system/paccache.service.d
install -m 0644 /workspace/system/systemd/system/paccache.service.d/retention.conf \
  /etc/systemd/system/paccache.service.d/retention.conf
install -m 0644 /workspace/system/systemd/system/paccache.service.d/retention.env \
  /etc/systemd/system/paccache.service.d/retention.env
systemd-analyze --man=no --recursive-errors=no verify /usr/lib/systemd/system/paccache.service
grep -Fx 'PACCACHE_ARGS=-k2' /etc/systemd/system/paccache.service.d/retention.env

# Validate every tracked Arch PKGBUILD recipe with makepkg --printsrcinfo.
pkg_scratch=/home/smoke/pkg-check
install -d -o smoke -g smoke "$pkg_scratch"
for pkg_dir in /workspace/packages/*; do
  [ -f "$pkg_dir/PKGBUILD" ] || continue
  pkg_name=$(basename "$pkg_dir")
  copy_dir="$pkg_scratch/$pkg_name"
  cp -a "$pkg_dir" "$copy_dir"
  cat > "$copy_dir/version.sh" <<'SH'
pkgver='1.0.0'
sha256sums_x86_64=('SKIP')
sha256sums_aarch64=('SKIP')
SH
  chown -R smoke:smoke "$copy_dir"
  srcinfo=$(runuser -u smoke -- sh -eu -c 'cd "$1" && makepkg --printsrcinfo' sh "$copy_dir")
  printf '%s\n' "$srcinfo" | grep -Fx '	pkgver = 1.0.0'
  printf '%s\n' "$srcinfo" | grep -Fx '	arch = x86_64'
  printf '%s\n' "$srcinfo" | grep -Fx '	arch = aarch64'
  printf '%s\n' "$srcinfo" | grep -Eq '^[[:space:]]+provides = '
  # Verify every local (non-URL) source file exists in the tracked recipe directory.
  printf '%s\n' "$srcinfo" | sed -n 's/^[[:space:]]*source = //p' | while IFS= read -r src; do
    case "$src" in
      *://*) ;;
      *) test -f "$pkg_dir/$src" ;;
    esac
  done
done
