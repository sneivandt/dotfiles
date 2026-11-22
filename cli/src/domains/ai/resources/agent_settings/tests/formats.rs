use super::*;

fn toml_resource(key: &str, value: toml::Value) -> AgentSettingResource {
    AgentSettingResource::new(
        "codex".to_string(),
        key.to_string(),
        value,
        SettingsFormat::Toml,
        PathBuf::from("/tmp/config.toml"),
    )
}

#[test]
fn json_state_reads_nested_path() {
    let document = serde_json::json!({ "footer": { "showBranch": true } });
    let resource = json_resource("footer.showBranch", toml::Value::Boolean(true));
    assert_eq!(
        resource.state_from_json_document(&document),
        ResourceState::Correct
    );

    let missing = json_resource("footer.showQuota", toml::Value::Boolean(false));
    assert_eq!(
        missing.state_from_json_document(&document),
        ResourceState::Missing
    );
}

#[test]
fn json_set_preserves_nested_siblings() {
    let resource = json_resource("footer.showBranch", toml::Value::Boolean(true));
    let before = r#"{"f\u006foter":{"showQuota":false,"extra":[{"$serde_json::private::Number":"},\""},1.2300e+02]}}"#;
    let updated = json::plan(before, &[resource]).unwrap().unwrap();
    let document: Value = serde_json::from_str(&updated).unwrap();
    assert_eq!(document["footer"]["showQuota"], serde_json::json!(false));
    assert_eq!(document["footer"]["showBranch"], serde_json::json!(true));
    assert_eq!(
        document["footer"]["extra"][0]["$serde_json::private::Number"],
        "},\""
    );
    assert!(updated.contains("1.2300e+02"), "{updated}");
}

#[test]
fn json_apply_preserves_unmanaged_numeric_tokens() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let path = dir.path().join("settings.json");
    let scale = format!("1.{}1", "0".repeat(17));
    let id = (u128::from(u64::MAX) + 2).to_string();
    let before =
        format!(r#"{{"footer":{{"showBranch":false,"scale":{scale}}},"integrationId":{id}}}"#);
    std::fs::write(&path, &before).unwrap();
    let document = AgentSettingsDocument::group(vec![AgentSettingResource::new(
        "copilot".into(),
        "footer.showBranch".into(),
        true.into(),
        SettingsFormat::Json,
        path.clone(),
    )])
    .unwrap()
    .pop()
    .unwrap();
    assert!(matches!(
        document.current_state().unwrap(),
        ResourceState::Incorrect { .. }
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert_eq!(document.apply().unwrap(), ResourceChange::Applied);

    let after = std::fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(parsed["footer"]["showBranch"], true);
    // Compare tokens, not numbers read back through the same lossy parser.
    let fields: Vec<_> = after
        .split(['{', '}', ','])
        .filter_map(|field| field.split_once(':'))
        .map(|(key, value)| (key.trim(), value.trim()))
        .collect();
    assert!(fields.contains(&(r#""scale""#, scale.as_str())), "{after}");
    assert!(
        fields.contains(&(r#""integrationId""#, id.as_str())),
        "{after}"
    );

    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH)
        .unwrap();
    let modified = path.metadata().unwrap().modified().unwrap();
    assert_eq!(document.current_state().unwrap(), ResourceState::Correct);
    assert_eq!(document.apply().unwrap(), ResourceChange::AlreadyCorrect);
    assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
}

#[test]
fn toml_edits_preserve_unmanaged_comments_layout_and_inline_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let before = "# My settings\nmodel = 'old' # preferred model\n\n# Integration notes\n[mcp_servers.example]\ncommand = 'tool' # do not change\noptions = { x = 1, y = 2 }\n";
    std::fs::write(&path, before).unwrap();
    let setting = AgentSettingResource::new(
        "codex".into(),
        "model".into(),
        "new".into(),
        SettingsFormat::Toml,
        path.clone(),
    );
    setting.apply().unwrap();
    let after = std::fs::read_to_string(path).unwrap();
    assert!(after.contains("# preferred model"));
    assert!(after.starts_with("# My settings\n"));
    assert!(after.ends_with("# Integration notes\n[mcp_servers.example]\ncommand = 'tool' # do not change\noptions = { x = 1, y = 2 }\n"));
}

#[test]
fn grouped_toml_change_preserves_comments_within_already_correct_settings() {
    let home = tempfile::tempdir_in(".").unwrap();
    let path = home.path().join("config.toml");
    let array = "tools = [\n  'lint', # leave this explanation here\n]\n";
    std::fs::write(&path, format!("{array}enabled = false\n")).unwrap();
    let document = AgentSettingsDocument::group(
        [
            ("tools", toml::Value::Array(vec!["lint".into()])),
            ("enabled", true.into()),
        ]
        .map(|(key, value)| {
            AgentSettingResource::new(
                "codex".into(),
                key.into(),
                value,
                SettingsFormat::Toml,
                path.clone(),
            )
        })
        .into(),
    )
    .unwrap()
    .pop()
    .unwrap();
    assert_eq!(document.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        format!("{array}enabled = true\n")
    );
    assert_eq!(document.apply().unwrap(), ResourceChange::AlreadyCorrect);
}

#[test]
fn replaces_existing_structured_values_in_inline_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let before = r#"skills = { config = [{ name = "documentation", enabled = false }], include_instructions = false } # skill preferences
shell_environment_policy = { set = { TERM = "dumb" }, inherit = "core" } # shell preferences
"#;
    let desired: toml::Table = toml::from_str(
        r#"
rules = [{ name = "documentation", enabled = false }, { name = "formatting", enabled = false }]
variables = {}
"#,
    )
    .unwrap();
    std::fs::write(&path, before).unwrap();
    let documents = AgentSettingsDocument::group(
        [
            ("skills.config", "rules"),
            ("shell_environment_policy.set", "variables"),
        ]
        .map(|(key, value)| {
            AgentSettingResource::new(
                "codex".into(),
                key.into(),
                desired[value].clone(),
                SettingsFormat::Toml,
                path.clone(),
            )
        })
        .into(),
    )
    .unwrap();
    let document = &documents[0];
    assert_eq!(document.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(document.current_state().unwrap(), ResourceState::Correct);

    let contents = std::fs::read_to_string(&path).unwrap();
    let updated: toml::Table = toml::from_str(&contents).unwrap();
    assert_eq!(updated["skills"]["config"], desired["rules"]);
    assert_eq!(
        updated["skills"]["include_instructions"].as_bool(),
        Some(false)
    );
    assert_eq!(
        updated["shell_environment_policy"]["set"],
        desired["variables"]
    );
    assert_eq!(
        updated["shell_environment_policy"]["inherit"].as_str(),
        Some("core")
    );
    assert!(contents.contains("# skill preferences"));
    assert!(contents.contains("# shell preferences"));
}

#[test]
fn non_object_json_documents_are_rejected_without_discarding_content() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let path = dir.path().join("settings.json");
    let resource = AgentSettingResource::new(
        "copilot".to_string(),
        "model".to_string(),
        toml::Value::String("fixture-model".to_string()),
        SettingsFormat::Json,
        path.clone(),
    );

    for contents in [
        "null",
        "[]",
        r#"[{"keepMe":"value"}]"#,
        "true",
        "42",
        r#""text""#,
    ] {
        std::fs::write(&path, contents).unwrap();
        assert!(resource.current_state().is_err(), "{contents}");
        assert!(resource.apply().is_err(), "{contents}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
    }
}

#[test]
fn toml_state_and_set_preserve_sibling_tables() {
    let mut document: toml::Table = toml::from_str(
        r#"
model = "gpt-5.5"

[mcp_servers.example]
command = "example"
"#,
    )
    .unwrap();
    let resource = toml_resource(
        "model_reasoning_effort",
        toml::Value::String("high".to_string()),
    );
    assert_eq!(
        resource.state_from_toml_document(&document),
        ResourceState::Missing
    );
    resource.set_in_toml_document(&mut document).unwrap();
    assert_eq!(
        document["mcp_servers"]["example"]["command"].as_str(),
        Some("example")
    );
    assert_eq!(
        resource.state_from_toml_document(&document),
        ResourceState::Correct
    );
}

#[test]
fn apply_json_writes_file_and_preserves_unmanaged_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{\n  \"keepMe\": \"value\"\n}\n").unwrap();
    let resource = AgentSettingResource::new(
        "copilot".to_string(),
        "model".to_string(),
        toml::Value::String("claude-opus-4.8".to_string()),
        SettingsFormat::Json,
        path.clone(),
    );

    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    let contents = std::fs::read_to_string(&path).unwrap();
    let document: Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(document["keepMe"], serde_json::json!("value"));
    assert_eq!(document["model"], serde_json::json!("claude-opus-4.8"));
    assert_eq!(
        contents,
        serde_json::to_string_pretty(&document).unwrap() + "\n"
    );
    resource.apply().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
}

#[test]
fn apply_toml_writes_file_and_preserves_unmanaged_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".codex").join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "model = \"gpt-5.5\"\n\n[mcp_servers.example]\ncommand = \"example\"\n",
    )
    .unwrap();
    let resource = AgentSettingResource::new(
        "codex".to_string(),
        "model_reasoning_effort".to_string(),
        toml::Value::String("high".to_string()),
        SettingsFormat::Toml,
        path.clone(),
    );

    assert_eq!(resource.apply().unwrap(), ResourceChange::Applied);
    assert_eq!(resource.current_state().unwrap(), ResourceState::Correct);
    let contents = std::fs::read_to_string(&path).unwrap();
    let document: toml::Table = toml::from_str(&contents).unwrap();
    assert_eq!(document["model"].as_str(), Some("gpt-5.5"));
    assert_eq!(
        document["mcp_servers"]["example"]["command"].as_str(),
        Some("example")
    );
    assert!(contents.ends_with('\n') && !contents.ends_with("\n\n"));
    resource.apply().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
}
