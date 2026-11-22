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

mod documents;
mod formats;
