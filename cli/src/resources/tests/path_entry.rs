use super::*;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn make_path_entry(home: &Path, on_path: bool) -> PathEntryResource {
    let executor: Arc<dyn Executor> = Arc::new(crate::exec::SystemExecutor);
    let platform = crate::platform::Platform {
        os: crate::platform::Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    PathEntryResource::new(home, platform, executor).with_path_source(on_path)
}

#[test]
fn description_includes_dir() {
    let r = make_path_entry(Path::new("/home/user"), false);
    let expected = Path::new(".local").join("bin");
    assert!(
        r.description()
            .contains(expected.to_string_lossy().as_ref()),
        "got: {}",
        r.description()
    );
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
fn correct_when_export_line_in_profile() {
    let tmp = TempDir::new().unwrap();
    let profile = tmp.path().join(".profile");
    std::fs::write(
        &profile,
        "# existing\nexport PATH=\"$HOME/.local/bin:$PATH\"\n",
    )
    .unwrap();

    let r = make_path_entry(tmp.path(), false);
    let state = r.current_state().unwrap();
    assert_eq!(state, ResourceState::Correct);
}

#[test]
fn apply_appends_to_profile() {
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
    assert!(content.starts_with("# existing config\n"));
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
fn apply_skips_when_profile_already_contains_export_line() {
    let tmp = TempDir::new().unwrap();
    let profile = tmp.path().join(".profile");
    let content = "# existing\nexport PATH=\"$HOME/.local/bin:$PATH\"\n";
    std::fs::write(&profile, content).unwrap();

    let r = make_path_entry(tmp.path(), false);
    let result = r.apply().unwrap();
    assert_eq!(result, ResourceChange::AlreadyCorrect);
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), content);
}

#[test]
fn remove_is_noop() {
    let r = make_path_entry(Path::new("/home/user"), false);
    let result = r.remove().unwrap();
    assert_eq!(result, ResourceChange::AlreadyCorrect);
}
