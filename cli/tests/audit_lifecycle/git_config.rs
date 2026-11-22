use super::*;

#[cfg(target_os = "linux")]
#[test]
fn rewriting_global_git_config_preserves_linux_access_metadata() {
    use rustix::fs::{XattrFlags, getxattr, setxattr};
    use std::os::unix::fs::MetadataExt as _;

    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{key='core.editor',value='vim'}]\n",
        )
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let config = cli.home.path().join(".gitconfig");
    std::fs::write(&config, "[core]\n editor = nano\n").unwrap();
    let mut acl = 2_u32.to_le_bytes().to_vec();
    let uid = config.metadata().unwrap().uid();
    for (tag, permissions, id) in [
        (1_u16, 6_u16, u32::MAX),
        (2, 4, uid),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(permissions.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let attributes = [
        ("system.posix_acl_access", acl.as_slice()),
        ("user.dotfiles_fixture", b"keep annotation".as_slice()),
    ];
    for (name, value) in attributes {
        setxattr(&config, name, value, XattrFlags::empty()).unwrap();
    }
    cli.run(None, "update", "git", true);
    assert!(std::fs::read_to_string(&config).unwrap().contains("vim"));
    for (name, value) in attributes {
        let mut observed = vec![0; value.len()];
        assert_eq!(
            getxattr(&config, name, observed.as_mut_slice()).unwrap(),
            value.len(),
            "{name}"
        );
        assert_eq!(observed, value, "{name}");
    }
}

#[cfg(unix)]
#[test]
fn global_git_update_honors_the_selected_file_and_later_conditional_includes() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{key='core.editor',value='vim'}]\n",
        )
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let default = cli.home.path().join(".gitconfig");
    let selected = cli.home.path().join("selected.gitconfig");
    let included = cli.home.path().join("included.gitconfig");
    let untouched_default = "[core]\n editor = existing-default\n";
    let untouched_include = "[core]\n editor = existing-override\n";
    std::fs::write(&default, untouched_default).unwrap();
    std::fs::write(&included, untouched_include).unwrap();
    std::fs::write(
        &selected,
        format!(
            "[core]\n editor = vim\n[includeIf \"gitdir:{}/.git\"]\n path = included.gitconfig\n",
            repo.root_path().display()
        ),
    )
    .unwrap();
    let git_update = || {
        let mut command = cli.command(None, "update", "git");
        command.env("GIT_CONFIG_GLOBAL", &selected).arg("--verbose");
        command
    };
    let mut command = git_update();
    let native_value = || {
        let result = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", cli.home.path())
            .env("GIT_CONFIG_GLOBAL", &selected)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(repo.root_path())
            .args(["config", "--get", "core.editor"])
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    assert_eq!(native_value(), "existing-override");
    let before = std::fs::read(&selected).unwrap();
    let preview = output(command.arg("--dry-run"), true);
    assert!(
        preview.contains("configure  core.editor = vim"),
        "{preview}"
    );
    assert_eq!(std::fs::read(&selected).unwrap(), before);

    let mut apply = git_update();
    output(&mut apply, true);
    assert_eq!(native_value(), "vim");
    assert_eq!(
        std::fs::read_to_string(&default).unwrap(),
        untouched_default
    );
    assert_eq!(
        std::fs::read_to_string(&included).unwrap(),
        untouched_include
    );
    let after = std::fs::read(&selected).unwrap();
    let text = output(&mut apply, true);
    assert!(text.contains("ok core.editor = vim"), "{text}");
    assert_eq!(std::fs::read(&selected).unwrap(), after);
}

#[test]
fn included_git_settings_converge_without_modifying_included_files() {
    let repo = common::TestContextBuilder::new()
        .with_config_file(
            "git-config.toml",
            "[base]\nsettings = [{ key = 'core.autocrlf', value = 'false' }]\n",
        )
        .build();
    let cli = LifecycleCli::new(repo.root_path());
    let config = cli.home.path().join(".gitconfig");
    let included = cli.home.path().join("included.gitconfig");
    std::fs::write(
        &config,
        "[core]\n autocrlf = true\n[include]\n path = included.gitconfig\n",
    )
    .unwrap();
    std::fs::write(&included, "[core]\n autocrlf = true\n").unwrap();
    let before = std::fs::read(&config).unwrap();
    output(cli.command(None, "update", "git").arg("--dry-run"), true);
    assert_eq!(std::fs::read(&config).unwrap(), before);
    cli.run(None, "update", "git", true);
    let after = std::fs::read(&config).unwrap();
    let text = cli.run(None, "update", "git", true);
    assert!(text.contains("Tasks · 1 unchanged"), "{text}");
    assert_eq!(std::fs::read(&config).unwrap(), after);
    assert_eq!(
        std::fs::read_to_string(&included).unwrap(),
        "[core]\n autocrlf = true\n"
    );
    assert_eq!(
        git2::Config::open(&config)
            .unwrap()
            .get_string("core.autocrlf")
            .unwrap(),
        "false"
    );
}

#[test]
fn git_update_lock_contention_retry_and_fresh_home_converge() {
    for include in [false, true] {
        let repo = common::TestContextBuilder::new()
            .with_config_file(
                "git-config.toml",
                "[base]\nsettings = [{ key = 'core.editor', value = 'fixture-editor' }]\n",
            )
            .build();
        let cli = LifecycleCli::new(repo.root_path());
        let path = cli.home.path().join(".gitconfig");
        let lock = cli.home.path().join(".gitconfig.lock");
        if include {
            std::fs::write(&path, "[include]\n path = included\n").unwrap();
            std::fs::write(
                cli.home.path().join("included"),
                "[core]\n editor = inherited\n",
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        std::fs::write(&lock, "foreign writer").unwrap();
        cli.run(None, "update", "git", false);
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "foreign writer");
        assert_eq!(
            path.exists(),
            include,
            "a failed first install must leave the config absent"
        );
        std::fs::remove_file(&lock).unwrap();
        cli.run(None, "update", "git", true);
        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            git2::Config::open(&path)
                .unwrap()
                .get_string("core.editor")
                .unwrap(),
            "fixture-editor"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        cli.run(None, "update", "git", true);
        assert_eq!(std::fs::read(&path).unwrap(), after);
        assert!(!lock.exists());
    }
}
