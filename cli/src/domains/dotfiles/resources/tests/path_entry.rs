//! Tests for dotfiles-domain resources.

use super::path_entry::*;
use crate::engine::{IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceState};
use crate::infra::exec::Executor;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn make_path_entry(home: &Path, on_path: bool) -> PathEntryResource {
    let executor: Arc<dyn Executor> = Arc::new(crate::infra::exec::MockExecutor::new());
    let platform = crate::infra::platform::Platform {
        os: crate::infra::platform::Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    PathEntryResource::new(
        home,
        platform,
        executor,
        crate::infra::env::MapEnv::new()
            .with(
                "PATH",
                if on_path {
                    home.join(".local/bin").into_os_string()
                } else {
                    "".into()
                },
            )
            .into_handle(),
    )
}

#[test]
fn description_includes_dir() {
    let r = make_path_entry(Path::new("/home/user"), false);
    assert_eq!(r.description(), "PATH \u{2192} ~/.local/bin");
}

#[test]
fn missing_when_on_path_but_profile_missing() {
    let tmp = TempDir::new().unwrap();
    let r = make_path_entry(tmp.path(), true);
    let state = r.current_state().unwrap();
    assert_eq!(state, ResourceState::Missing);
}

#[test]
fn missing_when_not_on_path_and_no_profile() {
    let tmp = TempDir::new().unwrap();
    let r = make_path_entry(tmp.path(), false);
    let state = r.current_state().unwrap();
    assert_eq!(state, ResourceState::Missing);
}

#[test]
fn unowned_or_malformed_exports_are_not_convergence_or_permission_to_rewrite() {
    let export = "export PATH=\"$HOME/.local/bin:$PATH\"";
    for content in [
        format!("# existing\n{export}\n"),
        format!("# user profile\r\n \t{export} \t\r\n"),
        format!(": <<'EXAMPLE'\n# Added by dotfiles\n{export}\nEXAMPLE\n"),
        // Even an exact marked trailer can be data and its closing delimiter.
        format!(": <<'{export}'\n# Added by dotfiles\n{export}\n"),
    ] {
        let dir = TempDir::new_in(".").unwrap();
        let profile = dir.path().join(".profile");
        std::fs::write(&profile, &content).unwrap();
        let resource = make_path_entry(dir.path(), false);
        assert!(
            resource
                .current_state()
                .unwrap_err()
                .to_string()
                .contains("unguarded")
        );
        assert!(resource.apply().is_err());
        assert_eq!(std::fs::read_to_string(profile).unwrap(), content);
    }
}

#[test]
fn inactive_profile_mentions_do_not_satisfy_path_state() {
    for existing in [
        "# export PATH=\"$HOME/.local/bin:$PATH\"\n",
        "# Example: export PATH=\"$HOME/.local/bin:$PATH\"\n",
        "printf '%s\\n' 'export PATH=\"$HOME/.local/bin:$PATH\"'\n",
    ] {
        let dir = TempDir::new_in(".").unwrap();
        let profile = dir.path().join(".profile");
        std::fs::write(&profile, existing).unwrap();
        let resource = make_path_entry(dir.path(), false);

        assert_eq!(
            resource.current_state().unwrap(),
            ResourceState::Missing,
            "inactive text must not count as the managed export: {existing:?}"
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        let installed = std::fs::read_to_string(&profile).unwrap();
        assert!(
            installed.contains(existing),
            "preserve existing profile text"
        );
        assert_eq!(resource.apply().unwrap(), ResourceChange::AlreadyCorrect);
        assert_eq!(std::fs::read_to_string(&profile).unwrap(), installed);
    }
}

#[test]
fn apply_preserves_profile_content_between_path_checks() {
    let tmp = TempDir::new().unwrap();
    let profile = tmp.path().join(".profile");
    std::fs::write(&profile, "# existing config\n").unwrap();

    let r = make_path_entry(tmp.path(), false);
    let result = r.apply().unwrap();
    assert_eq!(result, ResourceChange::Applied);

    let content = std::fs::read_to_string(&profile).unwrap();
    assert!(
        content.contains("# Added by dotfiles"),
        "missing marker in: {content}"
    );
    assert!(
        content.contains("export PATH=\"$HOME/.local/bin:$PATH\""),
        "missing export in: {content}"
    );
    assert!(content.contains("# existing config\n"));
    assert_eq!(content.matches("# Added by dotfiles").count(), 2);
}

#[test]
fn apply_creates_profile_if_missing() {
    let tmp = TempDir::new().unwrap();
    let profile = tmp.path().join(".profile");
    assert!(!profile.exists());

    let r = make_path_entry(tmp.path(), false);
    r.apply().unwrap();

    assert!(profile.exists());
    let content = std::fs::read_to_string(&profile).unwrap();
    assert!(content.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
}

#[test]
fn apply_appends_to_profile_when_on_path_but_profile_missing() {
    let tmp = TempDir::new().unwrap();
    let profile = tmp.path().join(".profile");
    let r = make_path_entry(tmp.path(), true);
    let result = r.apply().unwrap();
    assert_eq!(result, ResourceChange::Applied);

    let content = std::fs::read_to_string(&profile).unwrap();
    assert!(content.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
}

#[test]
fn remove_is_noop() {
    let r = make_path_entry(Path::new("/home/user"), false);
    let result = r.remove().unwrap();
    assert_eq!(result, ResourceChange::AlreadyCorrect);
}
