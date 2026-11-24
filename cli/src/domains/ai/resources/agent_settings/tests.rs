use super::*;

fn json_resource(key: &str, value: toml::Value) -> AgentSettingResource {
    AgentSettingResource::new(
        "copilot".to_string(),
        key.to_string(),
        value,
        SettingsFormat::Json,
        PathBuf::from("/tmp/settings.json"),
    )
}

#[cfg(target_os = "linux")]
#[test]
fn failed_staged_write_preserves_existing_document() {
    const CHILD_PATH: &str = "DOTFILES_TEST_LIMITED_SETTINGS_PATH";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let format = if std::env::var_os("DOTFILES_TEST_SETTINGS_TOML").is_some() {
            SettingsFormat::Toml
        } else {
            SettingsFormat::Json
        };
        let resource = AgentSettingResource::new(
            "fixture".into(),
            "managed".into(),
            toml::Value::String("x".repeat(65536)),
            format,
            path.into(),
        );
        assert!(
            resource.apply().is_err(),
            "file size limit must reject the replacement"
        );
        return;
    }
    for format in [SettingsFormat::Json, SettingsFormat::Toml] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings");
        let before = match format {
            SettingsFormat::Json => "{\"unmanaged\":true}",
            SettingsFormat::Toml => "unmanaged = true\n",
        };
        std::fs::write(&path, before).unwrap();
        let mut child = std::process::Command::new("sh");
        child.args(["-c", "trap '' XFSZ; ulimit -f 1; exec \"$@\"", "limited-settings-write"])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "domains::ai::resources::agent_settings::tests::failed_staged_write_preserves_existing_document"])
                // The file-size limit would truncate the instrumented child's profile.
                .env("LLVM_PROFILE_FILE", "/dev/null")
                .env(CHILD_PATH, &path);
        if format == SettingsFormat::Toml {
            child.env("DOTFILES_TEST_SETTINGS_TOML", "1");
        }
        let output = child.output().unwrap();
        assert!(output.status.success(), "limited writer failed: {output:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "failed replacement must clean staging files"
        );
    }
}

#[test]
fn agent_setting_and_document_state_claims_description_and_baseline_errors() {
    let dir = tempfile::tempdir().unwrap();

    // state_from_json_document & state_from_toml_document detect differing values.
    let json_path = dir.path().join("settings.json");
    std::fs::write(&json_path, r#"{"theme":"light"}"#).unwrap();
    let json_setting = AgentSettingResource::new(
        "copilot".into(),
        "theme".into(),
        toml::Value::String("dark".into()),
        SettingsFormat::Json,
        json_path.clone(),
    );
    assert_eq!(
        json_setting.current_state().unwrap(),
        ResourceState::Incorrect {
            current: "\"light\"".into(),
        }
    );
    assert_eq!(
        json_setting.access_claims(),
        vec![AccessClaim::exclusive(
            "agent-settings-document",
            json_path.as_os_str(),
        )]
    );

    let toml_path = dir.path().join("config.toml");
    std::fs::write(&toml_path, "model = \"old\"\n").unwrap();
    let toml_setting = AgentSettingResource::new(
        "codex".into(),
        "model".into(),
        toml::Value::String("new".into()),
        SettingsFormat::Toml,
        toml_path,
    );
    assert_eq!(
        toml_setting.current_state().unwrap(),
        ResourceState::Incorrect {
            current: "\"old\"".into(),
        }
    );

    // invalid_key rejects whitespace-only keys AND keys with empty segments.
    for bad_key in ["   ", "a..b", ".leading", "trailing."] {
        let invalid = json_resource(bad_key, toml::Value::Boolean(true));
        assert!(
            matches!(
                invalid.current_state().unwrap(),
                ResourceState::Invalid { .. }
            ),
            "expected key {bad_key:?} to be invalid"
        );
    }

    // read_baseline propagates non-NotFound errors (such as a directory path).
    let dir_target = dir.path().join("a-dir");
    std::fs::create_dir(&dir_target).unwrap();
    assert!(
        read_baseline(&dir_target).is_err(),
        "directory read error must not be treated as absent"
    );

    // AgentSettingsDocument claims and description.
    let docs = AgentSettingsDocument::group(vec![
        json_setting,
        AgentSettingResource::new(
            "copilot".into(),
            "telemetry".into(),
            toml::Value::Boolean(false),
            SettingsFormat::Json,
            json_path,
        ),
    ])
    .unwrap();
    assert_eq!(docs.len(), 1);
    let doc = &docs[0];
    assert_eq!(
        doc.access_claims(),
        vec![AccessClaim::exclusive(
            "agent-settings-document",
            doc.path.as_os_str(),
        )]
    );
    assert_eq!(
        doc.description(),
        format!("{} (2 managed keys)", doc.path.display())
    );
}

mod documents;
mod formats;
