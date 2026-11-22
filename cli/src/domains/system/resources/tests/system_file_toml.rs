use super::*;

#[test]
fn toml_merge_preserves_comments_and_layout_for_changed_and_unmanaged_keys() {
    let original = "# administrator heading\n\
            unmanaged = 'keep' # secret description\n\
            managed  = false # why this setting exists\n\n\
            # nested settings\n[service]\n\
            keep = { value = 1, other = 'retained' } # inline comment\n";
    let fragment = "managed = true\n[service]\nkeep = { value = 2 }\nnew = 'added'\n";
    let merged = merge_toml(original, fragment).unwrap();
    assert!(merged.starts_with("# administrator heading\n"));
    assert!(merged.contains("unmanaged = 'keep' # secret description\n"));
    assert!(merged.contains("managed  = true # why this setting exists\n"));
    assert!(merged.contains("# nested settings\n[service]\n"));
    assert!(merged.contains("other = 'retained'"));
    assert!(merged.contains("# inline comment"));
    assert!(contains_toml(&merged, fragment).unwrap());
    assert_eq!(merge_toml(&merged, fragment).unwrap(), merged);
}

#[test]
fn toml_convergence_preserves_nan_sign_and_exact_array_contents() {
    let fragment = "managed = nan\n[settings]\nvalues = [{ reading = -nan }]\n";
    let current = "managed = +nan\n[settings]\nvalues = [{ reading = -nan }]\nsibling = true\n";
    assert!(contains_toml(current, fragment).unwrap());
    for changed in [
        current.replace("managed = +nan", "managed = -nan"),
        current.replace("reading = -nan", "reading = nan"),
        current.replace("reading = -nan", "reading = -nan, extra = true"),
        current.replace("reading = -nan", "reading = 0.0"),
        current.replace("}]", "}, { reading = -nan }]"),
    ] {
        assert!(!contains_toml(&changed, fragment).unwrap(), "{changed}");
    }
}

#[cfg(unix)]
#[test]
fn private_toml_preparation_updates_an_array_of_tables_inside_inline_tables() {
    let fixture = PrivateStage::new("target");
    let original = "settings = { keep = 'retained', nested = { managed = [{ enabled = false }], sibling = 7 } }\n";
    let fragment = "[[settings.nested.managed]]\nenabled = true\n";
    fixture.write_target(original);

    assert_eq!(fixture.prepare(fragment, "toml").unwrap(), "prepared");
    let staged = fixture.staged_contents("input");
    assert!(contains_toml(&staged, fragment).unwrap(), "{staged}");
    let parsed = parse_toml(&staged).unwrap();
    assert_eq!(parsed["settings"]["keep"].as_str(), Some("retained"));
    assert_eq!(
        parsed["settings"]["nested"]["sibling"].as_integer(),
        Some(7)
    );
    assert_eq!(fixture.staged_contents("original"), original);
    assert_eq!(fixture.target_contents(), original);

    fs::rename(fixture.stage.join("input"), &fixture.target).unwrap();
    fs::remove_file(fixture.stage.join("original")).unwrap();
    assert_eq!(
        fixture.prepare(fragment, "toml").unwrap(),
        "already-correct"
    );
    assert!(fixture.stage_is_empty());
}
