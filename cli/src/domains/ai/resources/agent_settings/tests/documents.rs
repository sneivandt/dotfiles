use super::*;

#[test]
fn description_includes_target() {
    let resource = json_resource("model", toml::Value::String("claude-opus-4.8".to_string()));
    assert_eq!(
        resource.description(),
        "copilot:model = \"claude-opus-4.8\""
    );
}

#[test]
fn stale_document_baselines_and_newly_appeared_files_are_not_overwritten() {
    for before in [None, Some(b"{\"baseline\":true}".to_vec())] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{\"external\":\"edit\"}").unwrap();
        let setting = AgentSettingResource::new(
            "copilot".into(),
            "managed".into(),
            true.into(),
            SettingsFormat::Json,
            path.clone(),
        );
        let error = publish_document(
            &[setting],
            &document_path(&path).unwrap(),
            before.as_deref(),
            "{\"managed\":true}".into(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("changed during update"),
            "{error:#}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"external\":\"edit\"}"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn compatible_whole_parent_replaces_scalar_even_when_child_is_declared_first() {
    for (format, before) in [
        (SettingsFormat::Json, r#"{"parent":false}"#),
        (SettingsFormat::Toml, "parent = false\n"),
    ] {
        let home = tempfile::tempdir_in(".").unwrap();
        let path = home.path().join("doc");
        std::fs::write(&path, before).unwrap();
        let parent: toml::Table = toml::from_str("child = true").unwrap();
        let document = AgentSettingsDocument::group(
            [("parent.child", true.into()), ("parent", parent.into())]
                .map(|(key, value)| {
                    AgentSettingResource::new(
                        "fixture".into(),
                        key.into(),
                        value,
                        format,
                        path.clone(),
                    )
                })
                .into(),
        )
        .unwrap()
        .pop()
        .unwrap();
        assert_ne!(document.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(document.apply().unwrap(), ResourceChange::Applied);
        assert_eq!(document.current_state().unwrap(), ResourceState::Correct);
    }
}

#[test]
fn grouping_resolves_each_exact_spelling_once() {
    let paths = ["doc/file", "doc/./file", "doc/file", "doc//file"];
    let settings = paths.map(|path| AgentSettingResource {
        path: PathBuf::from(path),
        ..json_resource("enabled", true.into())
    });
    let mut lookups = Vec::new();
    let documents = AgentSettingsDocument::group_with(settings.into(), |path| {
        lookups.push(path.as_os_str().to_owned());
        Ok(PathBuf::from("resolved/file"))
    })
    .unwrap();
    assert_eq!(
        lookups,
        ["doc/file", "doc/./file", "doc//file"].map(std::ffi::OsString::from)
    );
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].settings.len(), paths.len());
}

#[cfg(any(unix, windows))]
#[test]
fn aliased_paths_share_a_single_document_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let alias = dir.path().join("alias.json");
    std::fs::write(&path, "{}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&path, &alias).unwrap();
    let settings = [(path.clone(), "one"), (alias.clone(), "two")].map(|(path, key)| {
        AgentSettingResource::new(
            "copilot".into(),
            key.into(),
            true.into(),
            SettingsFormat::Json,
            path,
        )
    });
    let documents = AgentSettingsDocument::group(settings.into()).unwrap();
    assert_eq!(documents.len(), 1);
    documents[0].apply().unwrap();
    let document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(document, serde_json::json!({"one":true,"two":true}));
    assert!(alias.symlink_metadata().unwrap().is_symlink());
    let conflicting = [(alias.clone(), true), (path.clone(), false)].map(|(destination, value)| {
        AgentSettingResource::new(
            "copilot".into(),
            "one".into(),
            value.into(),
            SettingsFormat::Json,
            destination,
        )
    });
    assert!(AgentSettingsDocument::group(conflicting.into()).is_err());

    // A later alias change must still block publication to the old target.
    let other = dir.path().join("other.json");
    std::fs::write(&path, "{}").unwrap();
    std::fs::write(&other, "{}").unwrap();
    std::fs::remove_file(&alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&other, &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&other, &alias).unwrap();
    let error = documents[0].apply().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("destination changed during update"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    assert_eq!(std::fs::read(&other).unwrap(), b"{}");
}

#[test]
fn incompatible_parent_values_fail_without_overwriting_either_document_format() {
    for (format, before, expected_error) in [
        (
            SettingsFormat::Json,
            "{ \"footer\": 42, \"unmanaged\": \"preserve formatting\" }\n",
            "not a JSON object",
        ),
        (
            SettingsFormat::Toml,
            "# preserve comment\nfooter = false\nunmanaged = \"preserve\"\n",
            "not a TOML table",
        ),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("settings");
        std::fs::write(&path, before).unwrap();
        let resource = AgentSettingResource::new(
            "fixture".into(),
            "footer.showBranch".into(),
            true.into(),
            format,
            path.clone(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        let error = resource.apply().unwrap_err();
        assert!(error.to_string().contains(expected_error), "{error}");
        assert!(error.to_string().contains("footer.showBranch"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before.as_bytes());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn unreadable_documents_are_errors_not_missing_settings() {
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("settings");
        std::fs::create_dir(&path).unwrap();
        let marker = path.join("keep");
        std::fs::write(&marker, "not a settings document").unwrap();
        let resource = AgentSettingResource::new(
            "fixture".into(),
            "managed".into(),
            true.into(),
            format,
            path.clone(),
        );

        assert!(resource.current_state().is_err());
        assert!(resource.apply().is_err());
        assert!(path.is_dir());
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "not a settings document"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
    }
}

#[test]
fn missing_and_empty_documents_converge_in_both_formats() {
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        for contents in [None, Some(""), Some(" \n\t\n")] {
            let dir = tempfile::tempdir_in(".").unwrap();
            let path = dir.path().join("settings");
            if let Some(contents) = contents {
                std::fs::write(&path, contents).unwrap();
            }
            let resource = AgentSettingResource::new(
                "fixture".to_string(),
                "nested.setting".to_string(),
                toml::Value::Boolean(true),
                format,
                path,
            );
            assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
            assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
            assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        }
    }
}

#[test]
fn invalid_documents_return_errors() {
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("settings.json");
    std::fs::write(&json_path, "{ not json").unwrap();
    let json = AgentSettingResource::new(
        "copilot".to_string(),
        "model".to_string(),
        toml::Value::String("gpt-5.6-sol".to_string()),
        SettingsFormat::Json,
        json_path.clone(),
    );
    assert!(json.current_state().is_err());
    assert!(json.apply().is_err());
    assert_eq!(std::fs::read_to_string(json_path).unwrap(), "{ not json");

    let toml_path = dir.path().join("config.toml");
    std::fs::write(&toml_path, "[not valid").unwrap();
    let toml = AgentSettingResource::new(
        "codex".to_string(),
        "model".to_string(),
        toml::Value::String("gpt-5.6-sol".to_string()),
        SettingsFormat::Toml,
        toml_path.clone(),
    );
    assert!(toml.current_state().is_err());
    assert!(toml.apply().is_err());
    assert_eq!(std::fs::read_to_string(toml_path).unwrap(), "[not valid");
}

#[test]
fn invalid_numeric_values_cannot_overwrite_documents_in_either_format() {
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        for value in [
            toml::Value::Float(f64::NAN),
            toml::Value::Array(vec![toml::Value::Float(f64::INFINITY)]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings");
            let before = match format {
                SettingsFormat::Json => "{\"unmanaged\":true}",
                SettingsFormat::Toml => "unmanaged = true\n",
            };
            std::fs::write(&path, before).unwrap();
            let resource = AgentSettingResource::new(
                "fixture".into(),
                "managed".into(),
                value,
                format,
                path.clone(),
            );
            assert!(matches!(
                resource.current_state().unwrap(),
                ResourceState::Invalid { .. }
            ));
            assert!(resource.apply().is_err());
            assert_eq!(std::fs::read_to_string(path).unwrap(), before);
        }
    }
}

#[cfg(unix)]
#[test]
fn broken_settings_links_fail_without_replacing_links_or_creating_referents() {
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let path = dir.path().join("settings");
        std::os::unix::fs::symlink("missing-settings", &path).unwrap();
        let resource = AgentSettingResource::new(
            "fixture".into(),
            "managed".into(),
            true.into(),
            format,
            path.clone(),
        );

        assert_eq!(resource.current_state().unwrap(), ResourceState::Missing);
        assert!(resource.apply().is_err());
        assert_eq!(
            std::fs::read_link(&path).unwrap(),
            PathBuf::from("missing-settings")
        );
        assert!(!dir.path().join("missing-settings").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn atomic_settings_updates_preserve_symlink_redirections_and_access_modes() {
    use std::os::unix::fs::PermissionsExt as _;
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("actual-settings");
        let path = dir.path().join("settings");
        let before = match format {
            SettingsFormat::Json => "{\"unmanaged\":true}",
            SettingsFormat::Toml => "unmanaged = true\n",
        };
        std::fs::write(&target, before).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink("actual-settings", &path).unwrap();
        let resource = AgentSettingResource::new(
            "fixture".into(),
            "managed".into(),
            true.into(),
            format,
            path.clone(),
        );
        resource.apply().unwrap();
        assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
        assert_eq!(
            std::fs::read_link(path).unwrap(),
            PathBuf::from("actual-settings")
        );
        assert_eq!(
            target.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
        let contents = std::fs::read_to_string(&target).unwrap();
        assert!(contents.contains("unmanaged"));
        assert!(contents.contains("managed"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
