use super::*;
use crate::infra::config::category_matcher::Category;
use crate::infra::config::test_helpers::{
    assert_mentions, assert_messages, rejects_toml, write_temp_toml,
};
use crate::infra::config::test_load_missing_returns_empty;
use crate::test_helpers::symlink;

rejects_toml! {
    "test.toml", |path| load(path, &[Category::Base]);
    unknown_key_in_symlink_table_is_rejected: r#"[base]
symlinks = [{ source = "bashrc", targett = ".bashrc" }]
"# => "targett";
    unknown_section_field_is_rejected: r#"[base]
symlink = ["bashrc"]
"# => "symlink";
    load_returns_error_on_malformed_toml: "[base\nsymlinks = [\"bashrc\"";
    load_returns_error_on_type_mismatch: "[base]\nsymlinks = \"not-an-array\"\n";
}

#[test]
fn load_base_symlinks() {
    for abs in ["/abs/path", "\\win\\path", "C:\\drive\\path", "D:rel"] {
        assert!(is_absolute_like(abs), "expected absolute-like: {abs}");
    }
    assert!(!is_absolute_like("relative/path"));
    assert!(!is_absolute_like("a"));

    let (_dir, path) = write_temp_toml(
        r#"[base]
symlinks = ["bashrc", "config/git/config"]

[desktop]
symlinks = ["config/i3"]
"#,
    );
    let symlinks: Vec<Symlink> = load(&path, &[Category::Base]).unwrap();
    assert_eq!(symlinks.len(), 2);
    assert_eq!(symlinks[0].source, "bashrc");
    assert_eq!(symlinks[1].source, "config/git/config");
}

#[test]
fn load_multi_category() {
    let (_dir, path) = write_temp_toml(
        r#"[base]
symlinks = ["bashrc"]

["arch-desktop"]
symlinks = ["config/i3"]
"#,
    );
    let symlinks: Vec<Symlink> =
        load(&path, &[Category::Base, Category::Arch, Category::Desktop]).unwrap();
    assert_eq!(symlinks.len(), 2);
}

#[test]
fn load_explicit_target_override() {
    let (_dir, path) = write_temp_toml(
        r#"[base]
symlinks = [
  "bashrc",
  { source = "config/something", target = ".custom-name" },
]
"#,
    );
    let symlinks: Vec<Symlink> = load(&path, &[Category::Base]).unwrap();
    assert_eq!(symlinks.len(), 2);
    assert_eq!(symlinks[0].source, "bashrc");
    assert!(symlinks[0].target.is_none());
    assert_eq!(symlinks[1].source, "config/something");
    assert_eq!(symlinks[1].target.as_deref(), Some(".custom-name"));
}

#[test]
fn load_allows_canonical_source_with_distinct_targets() {
    let (_dir, path) = write_temp_toml(
        r#"[base]
symlinks = ["config/shared"]

[windows]
symlinks = [
  { source = "config/shared", target = "AppData/Example/config" },
  { source = "config/shared", target = "Documents/Example/config" },
]
"#,
    );

    let symlinks = load(&path, &[Category::Base, Category::Windows]).unwrap();

    assert_eq!(symlinks.len(), 3);
    validate_unique_targets(&symlinks).unwrap();
}

#[test]
fn load_all_ignores_category_filters() {
    let (_dir, path) = write_temp_toml(
        r#"[base]
symlinks = ["bashrc"]

[desktop]
symlinks = ["config/i3"]

[windows]
symlinks = [{ source = "Documents/pwsh", target = "Documents/pwsh" }]
"#,
    );

    let symlinks = load_all(&path).unwrap();

    assert_eq!(symlinks.len(), 3);
    assert_eq!(symlinks[0].source, "bashrc");
    assert_eq!(symlinks[1].source, "config/i3");
    assert_eq!(symlinks[2].source, "Documents/pwsh");
}

test_load_missing_returns_empty!(load);

#[test]
fn validate_reports_missing_absolute_and_traversing_sources() {
    for (source, count, messages) in [
        ("nonexistent.txt", Some(1), &["does not exist"][..]),
        (
            "/absolute/path",
            Some(2),
            &["should be relative", "does not exist"],
        ),
        ("../outside", None, &["'..'"]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let diagnostics = validate(&[symlink(source, None)], root.path());
        assert_messages(&diagnostics, count, messages);
    }
}

#[test]
fn validation_rejects_paths_that_only_name_their_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("symlinks")).unwrap();
    std::fs::write(root.path().join("symlinks/file"), "content").unwrap();

    for path in ["", ".", "./", "././"] {
        for (source, target, code) in [
            (path, ".safe", "symlink.root-source"),
            ("file", path, "symlink.root-target"),
        ] {
            let entry = symlink(source, Some(target));
            assert!(validate_paths(&entry).is_err(), "{entry:?}");
            let diagnostics = validate(&[entry], root.path());
            assert_eq!(diagnostics.len(), 1, "{path:?}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code.to_string(), code);
            assert_eq!(diagnostics[0].source, SYMLINKS_TOML);
            assert_eq!(diagnostics[0].item, source);
            assert_eq!(
                diagnostics[0].severity,
                crate::infra::config::Severity::Error
            );
            assert!(diagnostics[0].message.contains("must name"));
        }
    }
}

#[test]
fn validation_accepts_named_dot_relative_paths_but_not_parents() {
    for (source, target) in [
        ("./bashrc", "./.bashrc"),
        ("./config/./git/config", ".config/./git/config"),
        (".hidden", "..hidden"),
    ] {
        validate_paths(&symlink(source, Some(target))).unwrap();
    }
    for (source, target) in [
        ("./../outside", ".safe"),
        ("file", "./../outside"),
        ("file", r".\..\outside"),
    ] {
        assert!(
            validate_paths(&symlink(source, Some(target))).is_err(),
            "{source} -> {target}"
        );
    }
}

#[test]
fn expand_glob_patterns_rejects_traversal_and_invalid_wildcards() {
    for (source, target, fragment) in [
        ("../*", "../../outside", "must not contain '..'"),
        ("skills/*", ".copilot/skills", "wildcard"),
        ("skills/**", ".copilot/skills/*", "**"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let entries = [symlink(source, Some(target))];
        let err = expand_glob_patterns(&entries, root.path()).unwrap_err();
        assert!(
            err.to_string().contains(fragment),
            "{source} -> {target}: {err}"
        );
    }
}

#[test]
fn multiple_glob_captures_preserve_order_sorting_and_origin() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let origin = fixture.path().join("origin");
    let fallback = fixture.path().join("fallback");
    for (root, relative) in [
        (&origin, "zeta/b.conf"),
        (&origin, "alpha/z.conf"),
        (&origin, "alpha/a.conf"),
        (&fallback, "wrong/ignored.conf"),
    ] {
        let path = root.join("symlinks").join("apps").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "fixture").unwrap();
    }
    let pattern = Symlink {
        source: "apps/*/*".into(),
        target: Some(".config/*/managed/*".into()),
        allow_external_target: true,
        origin: Some(origin.clone()),
    };

    let expanded = expand_glob_patterns(&[pattern], &fallback).unwrap();

    assert_eq!(
        expanded
            .iter()
            .map(|entry| (entry.source.as_str(), entry.target.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("apps/alpha/a.conf", Some(".config/alpha/managed/a.conf")),
            ("apps/alpha/z.conf", Some(".config/alpha/managed/z.conf")),
            ("apps/zeta/b.conf", Some(".config/zeta/managed/b.conf")),
        ],
    );
    assert!(
        expanded
            .iter()
            .all(|entry| entry.origin.as_ref() == Some(&origin))
    );
    assert!(expanded.iter().all(|entry| entry.allow_external_target));
}

#[test]
fn empty_matches_and_target_only_globs_are_errors_not_silent_omissions() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    std::fs::create_dir(fixture.path().join("symlinks")).unwrap();
    for (source, target, diagnostic) in [
        ("missing/*", None, "matched no entries"),
        ("file", Some(".config/*"), "is not a glob"),
    ] {
        let pattern = symlink(source, target);
        let error = expand_glob_patterns(&[pattern], fixture.path()).unwrap_err();
        assert!(error.to_string().contains(diagnostic), "{error}");
        assert!(error.to_string().contains(source), "{error}");
    }
}

#[test]
fn validate_rejects_absolute_and_traversing_targets() {
    for (target, fragment) in [
        ("/etc/passwd", "should be relative to $HOME directory"),
        ("../../etc/passwd", "'..'"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let sources = root.path().join("symlinks");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("bashrc"), "").unwrap();
        let warnings = validate(&[symlink("bashrc", Some(target))], root.path());
        assert_messages(&warnings, Some(1), &[fragment]);
    }
}

#[cfg(unix)]
#[test]
fn validate_detects_source_symlink_outside_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let outside_dir = tempfile::tempdir().unwrap();
    let symlinks_dir = temp_dir.path().join("symlinks");
    std::fs::create_dir_all(&symlinks_dir).unwrap();
    let outside = outside_dir.path().join("outside");
    std::fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, symlinks_dir.join("escaped")).unwrap();
    let symlink = symlink("escaped", None);

    let diagnostics = validate(std::slice::from_ref(&symlink), temp_dir.path());

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code.to_string(),
        "symlink.source-outside-root"
    );
    assert!(diagnostics[0].message.contains("outside"));
}

#[cfg(unix)]
#[test]
fn inaccessible_source_is_not_reported_as_missing_and_recovers_after_access_is_restored() {
    use crate::infra::config::Severity;
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = tempfile::tempdir().unwrap();
    let dir = fixture.path().join("symlinks/locked");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("config");
    std::fs::write(&source, "custom").unwrap();
    let entries = [symlink("locked/config", None)];
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let diagnostics = validate(&entries, fixture.path());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let [diagnostic] = diagnostics.as_slice() else {
        panic!("expected one source inspection error, got {diagnostics:?}");
    };
    assert_eq!(diagnostic.code, SYMLINK_SOURCE_INSPECTION);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.source, "symlinks.toml");
    assert_eq!(diagnostic.item, "locked/config");
    assert!(diagnostic.message.contains(&source.display().to_string()));
    assert!(validate(&entries, fixture.path()).is_empty());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "custom");
    std::fs::remove_file(source).unwrap();
    assert_eq!(
        validate(&entries, fixture.path())[0].code,
        SYMLINK_SOURCE_MISSING
    );
}

#[cfg(unix)]
#[test]
fn validate_allows_source_symlink_inside_root() {
    let temp_dir = tempfile::tempdir().unwrap();
    let symlinks_dir = temp_dir.path().join("symlinks");
    std::fs::create_dir_all(&symlinks_dir).unwrap();
    let canonical = symlinks_dir.join("canonical");
    std::fs::write(&canonical, "inside").unwrap();
    std::os::unix::fs::symlink(&canonical, symlinks_dir.join("alias")).unwrap();
    let symlink = symlink("alias", None);

    assert!(validate(&[symlink], temp_dir.path()).is_empty());
}

#[test]
fn validate_unique_targets_rejects_explicit_and_computed_collisions() {
    for declarations in [
        [("bashrc", None), ("other", Some(".bashrc"))],
        [("one", Some(".same")), ("two", Some(".same"))],
    ] {
        let root = tempfile::tempdir().unwrap();
        let sources = root.path().join("symlinks");
        std::fs::create_dir_all(&sources).unwrap();
        for (source, _) in declarations {
            std::fs::write(sources.join(source), "").unwrap();
        }
        let entries = declarations.map(|(source, target)| symlink(source, target));
        let error = validate_unique_targets(&entries).unwrap_err();
        assert!(
            error.to_string().contains("collision"),
            "{entries:?}: {error}"
        );
    }
}

#[test]
fn validate_unique_targets_rejects_ancestor_overlap_in_either_order() {
    let parent = symlink("config/example", None);
    let child = symlink("example-file", Some(".config/example/file"));

    for symlinks in [vec![parent.clone(), child.clone()], vec![child, parent]] {
        let error = validate_unique_targets(&symlinks).unwrap_err();
        assert_mentions(
            &error.to_string(),
            &["overlap", ".config/example", ".config/example/file"],
        );
    }
}

#[test]
fn validate_unique_targets_allows_shared_textual_prefix() {
    let symlinks = vec![
        symlink("one", Some(".config/example")),
        symlink("two", Some(".config/example-extra/file")),
    ];

    validate_unique_targets(&symlinks).unwrap();
}

#[test]
fn expand_glob_patterns_expands_skill_directories() {
    let temp_dir = tempfile::tempdir().unwrap();
    let skills_dir = temp_dir.path().join("symlinks").join("skills");
    std::fs::create_dir_all(skills_dir.join("alpha")).unwrap();
    std::fs::create_dir_all(skills_dir.join("bravo")).unwrap();

    let symlinks = vec![symlink("skills/*", Some(".copilot/skills/*"))];

    let expanded = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap();
    assert_eq!(expanded.len(), 2);
    assert_eq!(expanded[0].source, "skills/alpha");
    assert_eq!(expanded[0].target.as_deref(), Some(".copilot/skills/alpha"));
    assert_eq!(expanded[1].source, "skills/bravo");
    assert_eq!(expanded[1].target.as_deref(), Some(".copilot/skills/bravo"));
}

#[test]
fn expand_glob_patterns_preserves_origin() {
    let temp_dir = tempfile::tempdir().unwrap();
    let origin = temp_dir.path().join("overlay");
    std::fs::create_dir_all(origin.join("symlinks").join("skills").join("alpha")).unwrap();

    let symlinks = vec![Symlink {
        source: "skills/*".to_string(),
        target: Some(".copilot/skills/*".to_string()),
        allow_external_target: false,
        origin: Some(origin.clone()),
    }];

    let expanded = expand_glob_patterns(&symlinks, temp_dir.path()).unwrap();
    assert_eq!(expanded.len(), 1);
    assert_eq!(expanded[0].origin.as_deref(), Some(origin.as_path()));
}

#[test]
fn declaring_section_matches_exact_source_and_target_within_selected_categories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("conf")).unwrap();
    std::fs::write(
        dir.path().join("conf").join(SYMLINKS_TOML),
        r#"[base]
symlinks = [
  "bashrc",
  { source = "shared", target = ".first" },
]

[desktop]
symlinks = [
  { source = "shared", target = ".second" },
]
"#,
    )
    .unwrap();

    let base_and_desktop = [Category::Base, Category::Desktop];
    assert_eq!(
        declaring_section(&symlink("bashrc", None), dir.path(), &base_and_desktop).unwrap(),
        Some("base".into())
    );
    assert_eq!(
        declaring_section(
            &symlink("shared", Some(".second")),
            dir.path(),
            &base_and_desktop,
        )
        .unwrap(),
        Some("desktop".into())
    );
    // Same source with different target, or different source with same target, must not match.
    assert_eq!(
        declaring_section(
            &symlink("shared", Some(".third")),
            dir.path(),
            &base_and_desktop,
        )
        .unwrap(),
        None
    );
    assert_eq!(
        declaring_section(
            &symlink("other", Some(".first")),
            dir.path(),
            &base_and_desktop,
        )
        .unwrap(),
        None
    );
    // Unselected category is not matched.
    assert_eq!(
        declaring_section(
            &symlink("shared", Some(".second")),
            dir.path(),
            &[Category::Base],
        )
        .unwrap(),
        None
    );
}
