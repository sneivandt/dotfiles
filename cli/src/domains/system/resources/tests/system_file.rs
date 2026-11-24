use super::*;

#[cfg(unix)]
struct PrivateStage {
    root: tempfile::TempDir,
    target: PathBuf,
    stage: PathBuf,
    owner: u32,
}

#[cfg(unix)]
impl PrivateStage {
    fn new(target_name: &str) -> Self {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let root = tempfile::tempdir_in(fs::canonicalize(".").unwrap()).unwrap();
        let target = root.path().join(target_name);
        let stage = root.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = fs::metadata(root.path()).unwrap().uid();
        Self {
            root,
            target,
            stage,
            owner,
        }
    }

    fn prepare(&self, fragment: &str, strategy: &str) -> anyhow::Result<&'static str> {
        prepare_in_private_stage(&self.target, fragment, strategy, &self.stage, self.owner)
    }

    fn write_target(&self, contents: &str) {
        fs::write(&self.target, contents).unwrap();
    }

    fn target_contents(&self) -> String {
        fs::read_to_string(&self.target).unwrap()
    }

    fn staged_contents(&self, name: &str) -> String {
        fs::read_to_string(self.stage.join(name)).unwrap()
    }

    fn stage_is_empty(&self) -> bool {
        fs::read_dir(&self.stage).unwrap().next().is_none()
    }
}
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use std::path::Path;

fn entry(root: &Path, target: &Path, source: &str, merge: MergeStrategy) -> SystemFile {
    SystemFile {
        target: target.to_path_buf(),
        source: PathBuf::from(source),
        merge,
        origin: Some(root.to_path_buf()),
    }
}

fn fixture_resource(entry: SystemFile, executor: Arc<dyn Executor>) -> SystemFileResource {
    let root = entry.origin.clone().unwrap();
    let mut resource = SystemFileResource::new(entry, executor);
    resource.target_root = root;
    resource
}

#[test]
fn debug_format_and_non_create_only_invalid_installation_state() {
    let dir = tempfile::tempdir().unwrap();
    let source_dir = dir.path().join("system");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("bad.toml"), "[broken\n").unwrap();
    let target = dir.path().join("etc-bad.toml");
    let res = fixture_resource(
        entry(dir.path(), &target, "bad.toml", MergeStrategy::Toml),
        Arc::new(MockExecutor::new()),
    );
    let dbg = format!("{res:?}");
    assert!(dbg.contains("SystemFileResource"), "{dbg}");
    assert!(matches!(
        res.installation_state().unwrap(),
        ResourceState::Invalid { .. }
    ));
}

#[path = "system_file_assessment.rs"]
mod assessment;
#[path = "system_file_ini.rs"]
mod ini;
#[path = "system_file_pam.rs"]
mod pam;
#[cfg(unix)]
#[path = "system_file_preparation.rs"]
mod preparation;
#[path = "system_file_publication.rs"]
mod publication;
#[path = "system_file_toml.rs"]
mod toml;
