#[cfg(unix)]
use super::{
    Arc, CurrentFile, MergeStrategy, PathBuf, ResourceState, SystemFile, SystemFileResource,
};
use anyhow::bail;
#[cfg(unix)]
use anyhow::{Context as _, anyhow};
#[cfg(unix)]
use std::fs;
use std::io::Read as _;
#[cfg(unix)]
use std::io::Write as _;

// The privileged copy must finish on the destination filesystem before rename
// publishes it. A private directory prevents another user from replacing the
// staged file; the exit trap also removes a partial copy on command failure.
pub(super) const ATOMIC_INSTALL: &str = r#"set -eu
helper=$1
fragment=$3
strategy=$5
fragment_hash=$6
case "$helper" in /*) ;; *) helper="$PWD/$helper" ;; esac
case "$fragment" in /*) ;; *) fragment="$PWD/$fragment" ;; esac
owner=$(id -u)
check_directory() {
    if [ "$(find . -maxdepth 0 -uid "$owner" ! -perm /022 -print)" != . ]; then
        printf 'System file directory must be owned by the installer and writable only by its owner: %s\n' "$PWD" >&2
        exit 1
    fi
}
cd -P -- "$4"
check_directory
boundary=$PWD
parent=${2%/*}
case "$parent" in
    "$boundary") remaining= ;;
    "$boundary"/*) remaining=${parent#"$boundary"/} ;;
    *) printf 'System file target is outside %s: %s\n' "$boundary" "$2" >&2; exit 1 ;;
esac
# Enter each directory relative to the verified parent. A link introduced
# after discovery must never redirect mkdir, staging, or publication.
while [ -n "$remaining" ]; do
    component=${remaining%%/*}
    case "$component" in ''|.|..) exit 1 ;; esac
    next="$PWD/$component"
    if [ -L "./$component" ]; then
        printf 'System file parent became a symlink: %s\n' "$next" >&2
        exit 1
    fi
    if [ ! -d "./$component" ]; then
        mkdir -m 0755 -- "./$component"
    fi
    cd -P -- "./$component"
    if [ "$PWD" != "$next" ]; then
        printf 'System file parent changed while entering %s\n' "$next" >&2
        exit 1
    fi
    check_directory
    case "$remaining" in */*) remaining=${remaining#*/} ;; *) remaining= ;; esac
done
test "$(pwd -P)" = "$parent"
target="./${2##*/}"
stage_dir=$(mktemp -d -- ./.dotfiles-system-file.XXXXXXXXXX)
trap 'rm -rf -- "$stage_dir"' EXIT
trap 'exit 1' HUP INT TERM
result=$("$helper" system-file-helper --target "$PWD/${target#./}" \
    --fragment "$fragment" "--fragment-hash=$fragment_hash" \
    --merge "$strategy" --stage "$PWD/${stage_dir#./}")
case "$result" in
    already-correct) printf 'already-correct\n'; exit 0 ;;
    prepared) ;;
    *) printf 'Invalid system file helper response\n' >&2; exit 1 ;;
esac
set -- "$stage_dir/input" "$target" "$stage_dir/original"
replacement="$stage_dir/replacement"
install -D --owner=root --group=root --mode=0644 -- "$1" "$replacement"
if [ -L "$2" ] || { [ -e "$2" ] && [ ! -f "$2" ]; }; then
    printf 'Refusing to replace a non-regular system file: %s\n' "$2" >&2
    exit 1
fi
if [ -f "$3" ]; then
    # ctime covers owner, mode and xattrs; include inode identity for replacements.
    signature='%d:%i:%u:%g:%f:%z'
    before=$(stat -c "$signature" -- "$2")
    cp --attributes-only --preserve=mode,ownership,xattr -- "$2" "$replacement"
    expected=$(sha256sum < "$3")
    if [ -L "$2" ] || [ ! -f "$2" ] ||
        ! actual=$(sha256sum < "$2") || [ "$actual" != "$expected" ] ||
        ! after=$(stat -c "$signature" -- "$2") || [ "$after" != "$before" ]; then
        printf 'System file changed or could not be verified while preparing update: %s; retry the update\n' "$2" >&2
        exit 1
    fi
    test "$(pwd -P)" = "$parent"
    mv -fT -- "$replacement" "$2"
else
    # Publishing a new file must also fail if the target appears after discovery.
    test "$(pwd -P)" = "$parent"
    ln -T -- "$replacement" "$2"
fi
printf 'applied\n'
"#;

/// Run only in the scoped installer, without exposing destination content or diagnostics.
pub(crate) fn privileged_prepare(
    target: &std::path::Path,
    fragment_path: &std::path::Path,
    fragment_hash: &str,
    merge: &str,
    stage: &std::path::Path,
) -> anyhow::Result<&'static str> {
    #[cfg(unix)]
    {
        anyhow::ensure!(
            nix::unistd::Uid::effective().is_root(),
            "system file helper requires root"
        );
        anyhow::ensure!(
            target.starts_with("/etc"),
            "system file helper target must be below /etc"
        );
        let fragment = verified_fragment(fragment_path, fragment_hash)?;
        prepare_in_private_stage(target, &fragment, merge, stage, 0)
    }
    #[cfg(not(unix))]
    {
        let _ = (target, fragment_path, fragment_hash, merge, stage);
        bail!("system file helper requires Unix");
    }
}

pub(super) fn read_fragment(path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    crate::infra::fs::open_regular_source(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(unix)]
pub(super) fn verified_fragment(
    path: &std::path::Path,
    expected_hash: &str,
) -> anyhow::Result<String> {
    let bytes = read_fragment(path).map_err(|_| anyhow!("could not read system file fragment"))?;
    anyhow::ensure!(
        crate::infra::fs::content_hash(&bytes) == expected_hash,
        "system file fragment changed while preparing update; retry the update"
    );
    String::from_utf8(bytes).map_err(|_| anyhow!("system file fragment is not UTF-8"))
}

#[cfg(unix)]
pub(super) fn prepare_in_private_stage(
    target: &std::path::Path,
    fragment: &str,
    merge: &str,
    stage: &std::path::Path,
    owner: u32,
) -> anyhow::Result<&'static str> {
    prepare_contents(target, fragment, merge, stage, owner)
        .map_err(|_| anyhow!("system file preparation failed; destination was not published"))
}

#[cfg(unix)]
fn prepare_contents(
    target: &std::path::Path,
    fragment: &str,
    merge: &str,
    stage: &std::path::Path,
    owner: u32,
) -> anyhow::Result<&'static str> {
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

    let parent = target.parent().context("target has no parent")?;
    anyhow::ensure!(
        target.is_absolute()
            && dunce::canonicalize(parent)? == parent
            && stage.parent() == Some(parent),
        "unsafe helper paths"
    );
    for (path, private) in [(parent, false), (stage, true)] {
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.is_dir()
                && metadata.uid() == owner
                && metadata.mode() & if private { 0o077 } else { 0o022 } == 0,
            "unsafe staging directory"
        );
    }
    let merge = match merge {
        "toml" => MergeStrategy::Toml,
        "ini" => MergeStrategy::Ini,
        "pam" => MergeStrategy::Pam,
        "create-only" => MergeStrategy::CreateOnly,
        _ => bail!("unknown merge strategy"),
    };
    let resource = SystemFileResource::new(
        SystemFile {
            target: target.to_path_buf(),
            source: PathBuf::new(),
            merge,
            origin: None,
        },
        Arc::new(crate::infra::exec::ProcessExecutor::system()),
    );
    let current = SystemFileResource::current_content(target)?;
    match resource.state_for(&current, fragment) {
        ResourceState::Correct => return Ok("already-correct"),
        ResourceState::Invalid { reason } => bail!("{reason}"),
        ResourceState::Missing | ResourceState::Incorrect { .. } => {}
        ResourceState::Unknown { .. } => bail!("unknown system file state"),
    }
    let text = match &current {
        CurrentFile::Missing => "",
        CurrentFile::Present(text) => text,
        CurrentFile::NonRegular => bail!("not a regular file"),
    };
    let desired = resource.desired_content(text, fragment)?;
    for (name, content) in [
        ("input", Some(desired.as_str())),
        (
            "original",
            matches!(current, CurrentFile::Present(_)).then_some(text),
        ),
    ] {
        if let Some(content) = content {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(stage.join(name))?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
    }
    Ok("prepared")
}
