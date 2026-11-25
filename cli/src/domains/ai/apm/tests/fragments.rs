use super::*;

fn merge_test_fragments(contents: &[&str]) -> Result<String> {
    let dir = tempfile::tempdir().expect("create fragment directory");
    let paths: Vec<_> = contents
        .iter()
        .enumerate()
        .map(|(index, content)| {
            let path = dir.path().join(format!("{index}.yml"));
            std::fs::write(&path, content).expect("write fragment");
            path
        })
        .collect();
    merge_fragments(&paths)
}

#[test]
fn documented_agent_yaml_examples_use_the_real_fragment_merger() {
    let document = include_str!("../../../../../../docs/content/guides/agent-configuration.md");
    let examples: Vec<_> = document
        .split("```yaml")
        .skip(1)
        .map(|block| {
            let (_, body) = block.split_once('\n').expect("YAML fence header");
            body.split_once("```").expect("closed YAML fence").0
        })
        .collect();
    assert_eq!(
        examples.len(),
        2,
        "example plugin dependency and retained empty fragment"
    );
    for (index, example) in examples.iter().enumerate() {
        let merged = merge_test_fragments(&[example]).expect("merge documented fragment");
        let value: Value = crate::infra::yaml::from_str(&merged).expect("generated YAML");
        assert_eq!(value["name"], "dotfiles");
        assert_eq!(value["targets"].as_array().expect("targets").len(), 2);
        assert_eq!(
            value["dependencies"]["apm"]
                .as_array()
                .expect("APM dependencies")
                .len(),
            usize::from(index == 0)
        );
    }
}

#[test]
fn discover_fragment_files_returns_empty_when_dir_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(
        discover_fragment_files(dir.path())
            .expect("discover")
            .is_empty()
    );
}

#[test]
fn discover_fragment_files_errors_when_config_path_is_not_directory() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let apm_dir = dir.path().join(".apm");
    std::fs::create_dir_all(&apm_dir).expect("create ~/.apm");
    std::fs::write(apm_dir.join("config"), "not a directory\n").expect("write config file");

    let err = discover_fragment_files(dir.path()).expect_err("config file should error");
    assert!(
        format!("{err:#}").contains("reading APM config directory"),
        "expected context for read_dir failure, got {err:#}"
    );
}

#[test]
fn discover_fragment_files_returns_yaml_files_only_sorted() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let cfg = dir.path().join(".apm").join("config");
    std::fs::create_dir_all(&cfg).expect("create config dir");
    std::fs::write(cfg.join("work.yml"), "name: work\n").expect("write work.yml");
    std::fs::write(cfg.join("base.yaml"), "name: base\n").expect("write base.yaml");
    std::fs::write(cfg.join("README.md"), "ignore me\n").expect("write README.md");

    let files = discover_fragment_files(dir.path()).expect("discover");
    assert_eq!(files.len(), 2);
    assert!(files[0].ends_with("base.yaml"));
    assert!(files[1].ends_with("work.yml"));
}

#[test]
fn managed_fragment_shadows_only_the_same_target_identity() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".apm/config");
    std::fs::create_dir_all(&config).unwrap();
    let user = config.join("Team.yml");
    std::fs::write(&user, "description: home").unwrap();
    let selected = home.path().join("source.yml");
    std::fs::write(&selected, "description: selected").unwrap();
    let managed = [ApmFragmentSource::new(
        selected.clone(),
        ".apm/config/team.yml".into(),
    )];
    let files = discover_effective_fragment_files(home.path(), &managed, &[]).unwrap();
    if config.join("team.yml").try_exists().unwrap() {
        assert_eq!(files, [selected]);
    } else {
        assert_eq!(files, [user, selected]);
    }
}

fn check_coexisting_names(home: &Path, first: &str, second: &str) {
    let config = home.join(".apm/config");
    std::fs::create_dir_all(&config).unwrap();
    let selected = home.join("selected.yml");
    std::fs::write(&selected, "description: selected").unwrap();
    let managed = [ApmFragmentSource::new(
        selected.clone(),
        Path::new(".apm/config").join(first),
    )];
    std::fs::write(config.join(first), "description: old").unwrap();
    let user = config.join(second);
    std::fs::File::create_new(&user).expect("fixture names must physically coexist");
    assert_eq!(
        discover_effective_fragment_files(home, &managed, &[]).unwrap(),
        [selected, user]
    );
}

#[test]
fn coexisting_kelvin_and_ascii_fragments_both_contribute() {
    let home = tempfile::tempdir().unwrap();
    check_coexisting_names(home.path(), "K.yml", "\u{212a}.yml");
}

#[cfg(unix)]
#[test]
fn coexisting_unix_case_variants_both_contribute() {
    let home = tempfile::tempdir().unwrap();
    check_coexisting_names(home.path(), "Team.yml", "team.yml");
}

#[cfg(windows)]
#[test]
fn native_case_sensitive_fragment_names_and_parent_directories() {
    let home = tempfile::tempdir().unwrap();
    if !crate::infra::fs::enable_fixture_case_sensitivity(home.path()).unwrap() {
        return;
    }
    let canonical = home.path().join(".apm/config");
    let separate = home.path().join(".APM/Config");
    let other = [ApmFragmentSource::new(
        home.path().join("other-source.yml"),
        ".APM/Config/Team.yml".into(),
    )];
    // A different planned path is not selected even before its directories exist.
    assert!(
        discover_effective_fragment_files(home.path(), &other, &[])
            .unwrap()
            .is_empty()
    );
    std::fs::create_dir_all(&separate).unwrap();
    std::fs::create_dir_all(&canonical).unwrap();
    if !crate::infra::fs::enable_fixture_case_sensitivity(&canonical).unwrap() {
        return;
    }
    check_coexisting_names(home.path(), "Team.yml", "team.yml");
    let files = discover_effective_fragment_files(home.path(), &other, &[]).unwrap();
    assert_eq!(
        files,
        [canonical.join("Team.yml"), canonical.join("team.yml")]
    );
}

#[cfg(windows)]
#[test]
fn missing_fragment_directory_recognizes_its_ordinary_windows_alias() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("probe")).unwrap();
    if !home.path().join("PROBE").try_exists().unwrap() {
        return;
    }
    let source = home.path().join("selected.yml");
    let managed = [ApmFragmentSource::new(
        source.clone(),
        ".APM/Config/Team.yml".into(),
    )];
    assert_eq!(
        discover_effective_fragment_files(home.path(), &managed, &[]).unwrap(),
        [source]
    );
    assert!(!home.path().join(".apm").try_exists().unwrap());
}

#[test]
fn inactive_fragment_pointing_to_a_distinct_source_is_not_owned() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".apm/config");
    std::fs::create_dir_all(&config).unwrap();
    let retired = home.path().join("K.yml");
    let user = home.path().join("\u{212a}.yml");
    std::fs::write(&retired, "retired").unwrap();
    // Sharing file contents/identity is not sufficient to own a different entry.
    std::fs::hard_link(&retired, &user).unwrap();
    let link = config.join("old.yml");
    crate::infra::fs::create_native_symlink(&user, &link, false).unwrap();
    let original = std::fs::read_link(&link).unwrap();
    let inactive = [ApmFragmentSource::new(
        retired,
        ".apm/config/old.yml".into(),
    )];
    assert_eq!(
        discover_effective_fragment_files(home.path(), &[], &inactive).unwrap(),
        std::slice::from_ref(&link)
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), original);
}

#[test]
fn fragment_discovery_fails_on_an_uninspectable_parent() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".apm"), "preserve").unwrap();
    let managed = [ApmFragmentSource::new(
        home.path().join("selected"),
        ".apm/config/Team.yml".into(),
    )];
    assert!(discover_effective_fragment_files(home.path(), &managed, &[]).is_err());
    assert_eq!(
        std::fs::read(home.path().join(".apm")).unwrap(),
        b"preserve"
    );
}

#[test]
fn effective_fragments_replace_broken_managed_links_before_metadata_checks() {
    let dir = tempfile::tempdir_in(".").expect("create fixture directory");
    let home = crate::infra::fs::canonicalize(dir.path()).expect("resolve fixture home");
    let config = home.join(".apm").join("config");
    std::fs::create_dir_all(&config).expect("create fragment directory");
    let stale_link = config.join("base.yml");
    let missing = home.join("removed-source.yml");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&missing, &stale_link).expect("create stale fragment link");
    #[cfg(windows)]
    if std::os::windows::fs::symlink_file(&missing, &stale_link).is_err() {
        return;
    }

    let source = home.join("new-source.yml");
    std::fs::write(&source, "description: replacement\n").expect("write active source");
    let unmanaged = config.join("user.yml");
    std::fs::write(&unmanaged, "description: user\n").expect("write unmanaged fragment");

    assert!(discover_effective_fragment_files(&home, &[], &[]).is_err());
    let effective = discover_effective_fragment_files(
        &home,
        &[ApmFragmentSource::new(
            source.clone(),
            ".apm/config/base.yml".into(),
        )],
        &[],
    )
    .expect("managed source must mask the stale home entry");

    assert_eq!(effective, [source, unmanaged]);
    assert_eq!(std::fs::read_link(&stale_link).unwrap(), missing);

    let declared_name = if config.join("BASE.YML").symlink_metadata().is_ok() {
        "BASE.YML"
    } else {
        "base.yml"
    };
    let inactive = [ApmFragmentSource::new(
        missing.clone(),
        Path::new(".apm/config").join(declared_name),
    )];
    assert_eq!(
        discover_effective_fragment_files(&home, &[], &inactive).unwrap(),
        [config.join("user.yml")]
    );
    std::fs::write(&missing, "description: inactive desktop\n").unwrap();
    assert_eq!(
        discover_effective_fragment_files(&home, &[], &inactive).unwrap(),
        [config.join("user.yml")]
    );
    assert_eq!(std::fs::read_link(&stale_link).unwrap(), missing);
}

#[test]
fn merge_fragments_concatenates_apm_and_mcp_dependencies() {
    let merged = merge_test_fragments(&[
        "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n",
        "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - baz/qux\n  mcp:\n    - server-1\n",
    ])
    .expect("merge");
    assert!(merged.starts_with(GENERATED_HEADER));
    assert!(merged.contains("foo/bar"));
    assert!(merged.contains("baz/qux"));
    assert!(merged.contains("server-1"));
    assert!(merged.contains("name: dotfiles"));
}

#[test]
fn merge_fragments_deduplicates_apm_and_mcp_entries() {
    let merged = merge_test_fragments(&[
        "name: a\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: agency\n",
        "name: b\nversion: 1.0.0\ndependencies:\n  apm:\n    - foo/bar\n  mcp:\n    - name: kusto\n      command: other\n",
    ])
    .expect("merge");
    assert_eq!(merged.matches("foo/bar").count(), 1);
    assert_eq!(merged.matches("name: kusto").count(), 1);
    // First occurrence wins, so the duplicate's command is dropped.
    assert!(merged.contains("agency"));
    assert!(!merged.contains("other"));
}

#[test]
fn merge_fragments_deduplicates_dependency_sections_independently() {
    let fragment = "\
dependencies:
  apm:
    - example/plugin
devDependencies:
  apm:
    - example/plugin
";
    let merged = merge_test_fragments(&[fragment, fragment]).expect("merge");
    let document: Value = crate::infra::yaml::from_str(&merged).expect("parse merged manifest");
    for section in ["dependencies", "devDependencies"] {
        assert_eq!(
            document[section]["apm"],
            Value::Array(vec![Value::from("example/plugin")]),
            "{section} must retain its own deduplicated dependency"
        );
    }
}

#[cfg(unix)]
#[test]
fn discover_yaml_files_propagates_unreadable_config_directory_error() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir(&config_dir).unwrap();
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = discover_yaml_files(&config_dir);
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if nix::unistd::Uid::effective().is_root() {
        return;
    }
    assert!(
        result.is_err(),
        "permission error reading APM config dir must not be treated as absent"
    );
}

#[test]
fn merge_fragments_preserves_complex_map_entries() {
    let merged = merge_test_fragments(&[
        "name: x\nversion: 1.0.0\ndependencies:\n  apm:\n    - git: dev.azure.com/org/repo\n      path: services/foo\n",
    ])
    .expect("merge");
    assert!(merged.contains("dev.azure.com/org/repo"));
    assert!(merged.contains("services/foo"));
}

#[test]
fn merge_fragments_preserves_newer_manifest_fields() {
    let merged = merge_test_fragments(&[
        "\
name: a
version: 1.0.0
targets:
  - copilot
policy:
  fetch_failure_default: warn
registries:
  default: corp
  corp:
    url: https://packages.example.com
scripts:
  review: copilot -p review.prompt.md
dependencies:
  lsp:
    - rust-analyzer
devDependencies:
  apm:
    - ./dev/package
",
        "\
name: b
version: 1.0.0
targets:
  - copilot
  - claude
policy:
  hash: sha256:abcdef
scripts:
  start: copilot -p start.prompt.md
dependencies:
  lsp:
    - rust-analyzer
    - yaml-language-server
devDependencies:
  mcp:
    - name: fixture
      command: fixture-mcp
",
    ])
    .expect("merge");

    assert!(merged.contains("targets:"));
    assert_eq!(merged.matches("- copilot").count(), 1);
    assert!(merged.contains("- claude"));
    assert!(merged.contains("fetch_failure_default: warn"));
    assert!(merged.contains("hash: sha256:abcdef"));
    assert!(merged.contains("registries:"));
    assert!(merged.contains("review: copilot -p review.prompt.md"));
    assert!(merged.contains("start: copilot -p start.prompt.md"));
    assert!(merged.contains("lsp:"));
    assert_eq!(merged.matches("rust-analyzer").count(), 1);
    assert!(merged.contains("yaml-language-server"));
    assert!(merged.contains("devDependencies:"));
    assert!(merged.contains("./dev/package"));
    assert!(merged.contains("name: fixture"));
}

#[test]
fn merge_fragments_replaces_scalar_fields_with_later_fragments() {
    let merged = merge_test_fragments(&[
        "description: base description\n",
        "description: overlay description\n",
    ])
    .expect("merge");

    assert!(merged.contains("description: overlay description"));
    assert!(!merged.contains("description: base description"));
}

#[test]
fn merge_fragments_errors_when_dependency_section_is_not_mapping() {
    let err =
        merge_test_fragments(&["dependencies: []\n"]).expect_err("dependencies must be a mapping");

    assert!(
        format!("{err:#}").contains("dependencies in manifest fragment"),
        "expected dependency section context, got {err:#}"
    );
}

#[test]
fn merge_fragments_errors_when_dependency_group_is_not_sequence() {
    let err = merge_test_fragments(&["dependencies:\n  apm: foo/bar\n"])
        .expect_err("dependencies.apm must be a sequence");

    assert!(
        format!("{err:#}").contains("dependencies.apm"),
        "expected dependency group context, got {err:#}"
    );
}

#[test]
fn merge_fragments_skips_empty_files() {
    let merged = merge_test_fragments(&["\n\n"]).expect("merge");
    assert!(merged.contains("name: dotfiles"));
    assert!(!merged.contains("dependencies:"));
}
