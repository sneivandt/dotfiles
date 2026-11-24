//! Composition of per-domain configuration handles.
//!
//! The application layer loads the aggregate [`Config`] and then splits it into
//! one typed [`ConfigHandle`] per domain slice.  Each concrete task holds a
//! clone of exactly the handle it needs, so no task depends on the aggregate
//! configuration type. Handles remain immutable for the life of the process;
//! repository updates restart the command to publish a fresh snapshot.

use crate::app::config::Config;
use crate::domains::ai::apm::{ApmFragmentSource, InactiveApmFragments};
use crate::domains::files::config::symlinks::{
    Symlink, default_target, expand_optional_glob_patterns, path_identity, resolve_symlinks_dir,
    target_identity,
};
use crate::domains::packages::arch_recipes::ParuBase;
use crate::infra::ConfigHandle;
use std::path::Path;

macro_rules! define_config_store {
    ($($field:ident: $ty:ty, doc: $doc:literal, file: [$($file:path)?], decode: [$($decode:path)?] => $count:expr;)+) => {
        /// Shared immutable configuration split into per-domain handles.
        ///
        /// Cloning is cheap (each field is an `Arc`-backed [`ConfigHandle`]) and
        /// all clones observe the same startup snapshots.
        #[derive(Debug, Clone)]
        pub struct ConfigStore {
            /// Whole configuration, for app-owned validation tasks.
            pub aggregate: ConfigHandle<Config>,
            /// Resolved APM fragment sources derived from managed symlinks.
            pub(crate) apm_fragments: ConfigHandle<Vec<ApmFragmentSource>>,
            pub(crate) inactive_apm_fragments: ConfigHandle<InactiveApmFragments>,
            /// Selected source for the generated Paru configuration.
            pub(crate) paru_base: ConfigHandle<Option<ParuBase>>,
            $(
                #[doc = concat!("Configuration handle for `", stringify!($field), "`.")]
                pub $field: ConfigHandle<$ty>,
            )+
        }

        impl ConfigStore {
            /// Split an aggregate [`Config`] into per-domain handles.
            #[must_use]
            pub fn from_config(config: Config) -> Self {
                let apm_fragments = apm_fragment_sources(&config.symlinks, &config.root);
                let (expanded_validation_apm, inactive_apm_errors) =
                    expand_apm_validation_symlinks(&config.validation_symlinks, &config.root);
                let inactive_sources = apm_fragment_sources(&expanded_validation_apm, &config.root)
                    .into_iter()
                    .filter(|fragment| !apm_fragments.contains(fragment))
                    .collect();
                let inactive_apm_fragments =
                    InactiveApmFragments::new(inactive_sources, inactive_apm_errors);
                let paru_target = path_identity(Path::new(".config/paru/paru.conf.base"));
                let paru_base = config.symlinks.iter().find_map(|link| {
                    (target_identity(link) == paru_target)
                        .then(|| {
                            let root = resolve_symlinks_dir(link, &config.root);
                            ParuBase { path: root.join(&link.source), root }
                        })
                });
                Self {
                    $($field: ConfigHandle::new(config.$field.clone()),)+
                    apm_fragments: ConfigHandle::new(apm_fragments),
                    inactive_apm_fragments: ConfigHandle::new(inactive_apm_fragments),
                    paru_base: ConfigHandle::new(paru_base),
                    aggregate: ConfigHandle::new(config),
                }
            }
        }
    };
}

config_section_inventory!(define_config_store);

fn expand_apm_validation_symlinks(
    symlinks: &[Symlink],
    root: &Path,
) -> (Vec<Symlink>, Vec<String>) {
    let mut expanded = Vec::new();
    let mut errors = Vec::new();
    for symlink in symlinks {
        let is_glob = symlink.source.contains('*')
            || symlink.target.as_deref().is_some_and(|t| t.contains('*'));
        if !is_glob {
            if targets_apm_config(symlink) {
                expanded.push(symlink.clone());
            }
            continue;
        }
        if may_target_apm_config(symlink) {
            match expand_optional_glob_patterns(std::slice::from_ref(symlink), root) {
                Ok(matches) => expanded.extend(matches),
                Err(error) => errors.push(format!("{error:#}")),
            }
        }
    }
    (expanded, errors)
}

fn may_target_apm_config(symlink: &Symlink) -> bool {
    if targets_apm_config(symlink) {
        return true;
    }
    let target = symlink
        .target
        .clone()
        .unwrap_or_else(|| default_target(&symlink.source));
    Path::new(&target)
        .parent()
        .and_then(|parent| parent.to_str())
        .is_some_and(|parent| parent.contains('*'))
}

fn targets_apm_config(symlink: &Symlink) -> bool {
    let target = symlink
        .target
        .clone()
        .unwrap_or_else(|| default_target(&symlink.source));
    Path::new(&target)
        .parent()
        .is_some_and(|parent| path_identity(parent) == path_identity(Path::new(".apm/config")))
}

fn apm_fragment_sources(symlinks: &[Symlink], root: &Path) -> Vec<ApmFragmentSource> {
    symlinks
        .iter()
        .filter_map(|symlink| {
            let target = apm_fragment_target(symlink)?;
            let source = resolve_symlinks_dir(symlink, root).join(&symlink.source);
            Some(ApmFragmentSource::new(source, target))
        })
        .collect()
}

fn apm_fragment_target(symlink: &Symlink) -> Option<std::path::PathBuf> {
    let target = symlink
        .target
        .clone()
        .unwrap_or_else(|| default_target(&symlink.source));
    let path = Path::new(&target);
    if path_identity(path.parent()?) != path_identity(Path::new(".apm/config")) {
        return None;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        })
        .then(|| {
            path.components()
                .filter(|part| *part != std::path::Component::CurDir)
                .collect()
        })
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "test code uses direct indexing for focused assertions"
)]
mod tests {
    use super::*;
    use crate::domains::files::config::symlinks::Symlink;
    use crate::test_helpers::empty_config;
    use std::path::PathBuf;

    #[test]
    fn paru_generation_uses_the_selected_overlay_before_links_exist() {
        use crate::domains::packages::arch_recipes::ConfigureParu;
        use crate::engine::Task as _;
        use crate::infra::env::MapEnv;
        use crate::test_helpers::{make_linux_context, task_batch};

        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("main");
        let overlay = fixture.path().join("overlay");
        let home = fixture.path().join("home");
        let public = root.join("symlinks/config/paru/paru.conf.base");
        let selected = overlay.join("symlinks/custom-paru");
        for (path, content) in [
            (&public, "[options]\nBottomUp\n"),
            (&selected, "[options]\nNewsOnUpgrade\n"),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let mut config = empty_config(root.clone());
        config.symlinks = vec![Symlink {
            source: "custom-paru".into(),
            target: Some("./.config/paru/paru.conf.base".into()),
            origin: Some(overlay.clone()),
            allow_external_target: false,
        }];
        let store = ConfigStore::from_config(config);
        let task = ConfigureParu::new(store.paru_base);
        let ctx = make_linux_context(root, Some(overlay))
            .with_home(home.clone())
            .with_env(MapEnv::new().into_handle());
        let target = home.join(".config/paru/paru.conf");
        assert_eq!(
            task_batch(&task.run(&ctx.with_dry_run(true)).unwrap()).changed_count(),
            1
        );
        assert!(!target.exists());
        assert_eq!(task_batch(&task.run(&ctx).unwrap()).changed_count(), 1);
        let generated = std::fs::read_to_string(&target).unwrap();
        assert!(generated.contains("NewsOnUpgrade"));
        assert!(!generated.contains("BottomUp"));
        std::fs::remove_file(&selected).unwrap();
        assert!(
            task.run(&ctx).is_err(),
            "a missing selected source must not fall back to public"
        );
        assert_eq!(std::fs::read_to_string(target).unwrap(), generated);
    }

    fn symlink(root: &Path, source: &str) -> Symlink {
        Symlink {
            source: source.to_string(),
            target: None,
            allow_external_target: false,
            origin: Some(root.to_path_buf()),
        }
    }

    #[test]
    fn expands_glob_declared_inactive_apm_fragments() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let missing_overlay = fixture.path().join("absent-overlay");
        let fragment_dir = root.join("symlinks/apm/config");
        std::fs::create_dir_all(&fragment_dir).unwrap();
        std::fs::write(fragment_dir.join("base.yml"), "name: base\n").unwrap();
        std::fs::write(fragment_dir.join("desktop.yml"), "name: desktop\n").unwrap();

        let mut config = empty_config(root.clone());
        config.symlinks = vec![symlink(&root, "apm/config/base.yml")];
        config.validation_symlinks = vec![
            symlink(&root, "apm/config/*"),
            symlink(&missing_overlay, "apm/config/*"),
        ];

        let store = ConfigStore::from_config(config);

        assert_eq!(
            *store.apm_fragments.get(),
            vec![ApmFragmentSource::new(
                fragment_dir.join("base.yml"),
                ".apm/config/base.yml".into(),
            )]
        );
        assert_eq!(
            store.inactive_apm_fragments.get().sources().unwrap(),
            &[ApmFragmentSource::new(
                fragment_dir.join("desktop.yml"),
                ".apm/config/desktop.yml".into(),
            )]
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_enumerable_inactive_apm_glob_surfaces_error_before_reconciliation() {
        use crate::domains::ai::apm::{ApmPackageMode, InstallApmPackages};
        use crate::engine::Task as _;
        use crate::test_helpers::make_linux_context;
        use std::os::unix::fs::PermissionsExt as _;

        struct RestoreMode(PathBuf);
        impl Drop for RestoreMode {
            fn drop(&mut self) {
                drop(std::fs::set_permissions(
                    &self.0,
                    std::fs::Permissions::from_mode(0o755),
                ));
            }
        }

        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let home = fixture.path().join("home");
        let missing_overlay = fixture.path().join("absent-overlay");
        let fragment_dir = root.join("symlinks/apm/config");
        let inactive_source = fragment_dir.join("desktop.yml");
        let home_config = home.join(".apm/config");
        std::fs::create_dir_all(&fragment_dir).unwrap();
        std::fs::create_dir_all(&home_config).unwrap();
        std::fs::write(
            &inactive_source,
            "dependencies:\n  apm:\n    - inactive/plugin\n",
        )
        .unwrap();
        let retained_link = home_config.join("desktop.yml");
        std::os::unix::fs::symlink(&inactive_source, &retained_link).unwrap();

        std::fs::set_permissions(&fragment_dir, std::fs::Permissions::from_mode(0o111)).unwrap();
        let _restore = RestoreMode(fragment_dir.clone());
        if std::fs::read_dir(&fragment_dir).is_ok() {
            return;
        }
        // Traversing the 0o111 directory still lets the retained symlink be read.
        assert!(
            std::fs::read_to_string(&retained_link)
                .unwrap()
                .contains("inactive/plugin")
        );

        let mut config = empty_config(root.clone());
        config.validation_symlinks = vec![
            symlink(&root, "apm/config/*"),
            symlink(&missing_overlay, "apm/config/*"),
        ];
        let store = ConfigStore::from_config(config);
        let inspection_err = store
            .inactive_apm_fragments
            .get()
            .sources()
            .unwrap_err()
            .to_string();
        assert!(
            inspection_err.contains("could not inspect inactive APM fragment sources"),
            "{inspection_err}"
        );

        let task = InstallApmPackages::new(store.apm_fragments, ApmPackageMode::Install)
            .with_inactive_fragments(store.inactive_apm_fragments);
        let ctx = make_linux_context(root, None).with_home(home.clone());

        assert!(task.should_run(&ctx));
        for dry_run in [true, false] {
            let err = task.run(&ctx.with_dry_run(dry_run)).unwrap_err();
            let rendered = format!("{err:#}");
            assert!(
                rendered.contains("could not inspect inactive APM fragment sources")
                    && rendered.contains("reading directory"),
                "dry_run={dry_run}: {rendered}"
            );
            assert!(
                !home.join(".apm/apm.yml").exists(),
                "inactive dependencies must not be reconciled into apm.yml"
            );
        }
    }

    #[test]
    fn derives_apm_fragments_from_managed_symlinks() {
        let root = PathBuf::from("/repo");
        let mut config = empty_config(root.clone());
        config.symlinks = vec![
            symlink(&root, "apm/config/base.yml"),
            symlink(&root, "apm/plugins/dot-agent"),
        ];

        let store = ConfigStore::from_config(config);

        assert_eq!(
            *store.apm_fragments.get(),
            vec![ApmFragmentSource::new(
                root.join("symlinks").join("apm/config/base.yml"),
                ".apm/config/base.yml".into(),
            )]
        );
    }

    #[test]
    fn fragment_sources_follow_normalized_managed_targets() {
        let root = PathBuf::from("fixture-root");
        for (source, target, expected) in [
            ("./apm/config/base.yml", None, Some(".apm/config/base.yml")),
            (
                "apm/./config/base.yaml",
                None,
                Some(".apm/config/base.yaml"),
            ),
            (
                "fragment.yml",
                Some("./.apm/config/base.yml"),
                Some(".apm/config/base.yml"),
            ),
            (
                "fragment.yml",
                Some(".apm/./config//base.yml"),
                Some(".apm/config/base.yml"),
            ),
            ("fragment.yml", Some(".apm/config/nested/base.yml"), None),
            ("fragment.yml", Some("../.apm/config/base.yml"), None),
            ("fragment.yml", Some(".apm/config/base.json"), None),
            (
                "fragment.yml",
                Some(".APM/Config/Office.YML"),
                cfg!(windows).then_some(".APM/Config/Office.YML"),
            ),
        ] {
            let mut config = empty_config(root.clone());
            config.symlinks = vec![Symlink {
                source: source.into(),
                target: target.map(str::to_owned),
                allow_external_target: false,
                origin: Some(root.clone()),
            }];
            let store = ConfigStore::from_config(config);
            let expected = expected
                .map(|expected_path| {
                    ApmFragmentSource::new(root.join("symlinks").join(source), expected_path.into())
                })
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(
                *store.apm_fragments.get(),
                expected,
                "{source:?} -> {target:?}"
            );
        }

        let plain = symlink(&root, "bashrc");
        assert!(!targets_apm_config(&plain));
        assert!(!may_target_apm_config(&plain));
        let apm_entry = symlink(&root, "apm/config/base.yml");
        assert!(targets_apm_config(&apm_entry));
        assert!(may_target_apm_config(&apm_entry));
        let wildcard = Symlink {
            source: "wild/*".into(),
            target: Some("*/base.yml".into()),
            allow_external_target: false,
            origin: Some(root),
        };
        assert!(!targets_apm_config(&wildcard));
        assert!(may_target_apm_config(&wildcard));
    }

    #[cfg(unix)]
    #[test]
    fn backslashes_in_unix_targets_are_not_apm_directory_separators() {
        let mut config = empty_config("fixture-root".into());
        config.symlinks = vec![Symlink {
            source: "fragment.yml".into(),
            target: Some(r".apm\config\base.yml".into()),
            allow_external_target: false,
            origin: None,
        }];

        assert!(
            ConfigStore::from_config(config)
                .apm_fragments
                .get()
                .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn backslashes_in_windows_targets_are_apm_directory_separators() {
        for (source, target) in [
            (r".\apm\config\base.yml", None),
            ("fragment.yml", Some(r".\.apm\config\base.yml")),
        ] {
            let entry = Symlink {
                source: source.into(),
                target: target.map(str::to_owned),
                allow_external_target: false,
                origin: None,
            };
            assert_eq!(
                apm_fragment_target(&entry),
                Some(".apm/config/base.yml".into()),
                "{source:?} -> {target:?}"
            );
        }
    }
}
