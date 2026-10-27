use super::Symlink;
use anyhow::{Result, bail};
use std::path::{Component, Path};

pub(super) fn validate_unique_targets(symlinks: &[Symlink]) -> Result<()> {
    let mut targets: Vec<(String, Vec<String>, String)> = Vec::new();
    for symlink in symlinks {
        let target = target_key(symlink);
        let segments = target_identity(symlink);
        for (existing_target, existing_segments, existing_source) in &targets {
            if existing_segments == &segments {
                bail!(
                    "symlink target collision for '{target}': '{existing_source}' and '{}' both map to the same target",
                    symlink.source
                );
            }

            if is_ancestor(existing_segments, &segments) {
                bail!(
                    "symlink target overlap: '{existing_source}' maps to parent target '{existing_target}', which contains target '{target}' for '{}'",
                    symlink.source
                );
            }
            if is_ancestor(&segments, existing_segments) {
                bail!(
                    "symlink target overlap: '{}' maps to parent target '{target}', which contains target '{existing_target}' for '{existing_source}'",
                    symlink.source
                );
            }
        }
        targets.push((target, segments, symlink.source.clone()));
    }
    Ok(())
}

pub(crate) fn target_identity(symlink: &Symlink) -> Vec<String> {
    Path::new(&target_key(symlink))
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .map(|component| {
            let segment = component.as_os_str().to_string_lossy();
            #[cfg(windows)]
            {
                segment.to_lowercase()
            }
            #[cfg(not(windows))]
            {
                segment.into_owned()
            }
        })
        .collect()
}

fn is_ancestor(parent: &[String], child: &[String]) -> bool {
    parent.len() < child.len() && child.starts_with(parent)
}

fn target_key(symlink: &Symlink) -> String {
    symlink
        .target
        .clone()
        .unwrap_or_else(|| super::default_target(&symlink.source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::symlink;

    #[test]
    fn dot_aliases_collide_with_explicit_and_default_targets() {
        for pair in [
            (symlink("./bashrc", None), symlink("other", Some(".bashrc"))),
            (symlink("bashrc", None), symlink("././bashrc", None)),
            (
                symlink("one", Some("./.config/example")),
                symlink("two", Some(".config/./example/.")),
            ),
        ] {
            let entries: [Symlink; 2] = pair.into();
            let error = validate_unique_targets(&entries).unwrap_err();
            assert!(error.to_string().contains("collision"), "{error}");
        }
    }

    #[test]
    fn dot_aliases_overlap_in_either_order() {
        for child in [
            "./.config/example/settings",
            ".config/./example/settings",
            ".config/example/./settings",
            ".config//example/settings",
        ] {
            let parent = symlink("./config/example", None);
            let child = symlink("child", Some(child));
            for entries in [[parent.clone(), child.clone()], [child, parent]] {
                let error = validate_unique_targets(&entries).unwrap_err();
                assert!(error.to_string().contains("overlap"), "{error}");
            }
        }
    }

    #[test]
    fn case_aliases_follow_platform_semantics() {
        for (target, kind) in [
            (".CONFIG/EXAMPLE", "collision"),
            (".CONFIG/EXAMPLE/settings", "overlap"),
        ] {
            let parent = symlink("config/example", None);
            let other = symlink("other", Some(target));
            for entries in [[parent.clone(), other.clone()], [other, parent]] {
                let result = validate_unique_targets(&entries);
                if cfg!(windows) {
                    let error = result.unwrap_err();
                    assert!(error.to_string().contains(kind), "{error}");
                } else {
                    result.unwrap();
                }
            }
        }
    }

    #[test]
    fn backslash_aliases_follow_platform_semantics() {
        let entries = [
            symlink("config/example", None),
            symlink("child", Some(r".config\.\example\settings")),
        ];
        let result = validate_unique_targets(&entries);
        if cfg!(windows) {
            assert!(result.unwrap_err().to_string().contains("overlap"));
        } else {
            result.unwrap();
        }
    }

    #[test]
    fn normalized_siblings_and_similar_names_remain_distinct() {
        validate_unique_targets(&[
            symlink("one", Some(".config/./example/one")),
            symlink("two", Some("./.config/example/two")),
            symlink("three", Some(".config/./example-extra")),
        ])
        .unwrap();
    }
}
