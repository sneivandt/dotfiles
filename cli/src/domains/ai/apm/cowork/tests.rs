use std::sync::Arc;

use super::*;
use crate::domains::ai::apm::targets::ONEDRIVE_COMMERCIAL;
use crate::domains::ai::apm::test_fixture::make_context_with_home;
use crate::infra::env::MapEnv;
use crate::infra::exec::MockExecutor;
use crate::infra::platform::{Os, Platform};

fn setup_skill(home: &Path, target_subset: &str) -> (PathBuf, PathBuf, Context) {
    let onedrive = home.join("OneDrive - Test");
    let source_skill = home.join(".agents").join("skills").join("example");
    let target_skill = onedrive
        .join("Documents")
        .join("Cowork")
        .join("skills")
        .join("example");
    std::fs::create_dir_all(&source_skill).expect("create source skill");
    std::fs::create_dir_all(&target_skill).expect("create target skill");
    std::fs::create_dir_all(home.join(".apm")).expect("create APM directory");
    std::fs::write(
        home.join(".apm").join("apm.lock.yaml"),
        format!(
            "dependencies:\n  - deployed_files:\n      - \
                 .agents/skills/example/SKILL.md\n    target_subset: {target_subset}\n"
        ),
    )
    .expect("write lock");
    std::fs::write(source_skill.join("SKILL.md"), "current").expect("write source");
    let ctx = make_context_with_home(home, Platform::new(Os::Windows, false), MockExecutor::new())
        .with_env(Arc::new(MapEnv::new().with(ONEDRIVE_COMMERCIAL, &onedrive)));
    (source_skill, target_skill, ctx)
}

#[cfg(target_os = "linux")]
#[test]
fn interrupted_copy_keeps_the_existing_file_and_cleans_staging() {
    const CHILD_ROOT: &str = "DOTFILES_TEST_COWORK_COPY_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        assert!(copy_skill_files(&root.join("source"), &root.join("target")).is_err());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("source"), "x".repeat(65536)).unwrap();
    std::fs::write(dir.path().join("target"), "previous deployment").unwrap();
    let output = std::process::Command::new("sh")
            .args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "limited-cowork-copy"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", "domains::ai::apm::cowork::tests::interrupted_copy_keeps_the_existing_file_and_cleans_staging"])
            .env(CHILD_ROOT, dir.path()).env("LLVM_PROFILE_FILE", "/dev/null")
            .output().unwrap();
    assert!(output.status.success(), "limited copy failed: {output:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("target")).unwrap(),
        "previous deployment"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

mod legacy;
mod links;
mod ownership;
mod reconciliation;
