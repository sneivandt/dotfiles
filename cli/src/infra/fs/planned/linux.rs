use super::PlannedDirectory;
use anyhow::{Context as _, Result};
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

fn directory_id(path: &Path) -> Result<Option<(u64, u64)>> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(meta.is_dir().then(|| (meta.dev(), meta.ino()))),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(err) => Err(err).with_context(|| format!("inspect directory {}", path.display())),
    }
}

pub(super) fn aliased_trees_overlap(a: &PlannedDirectory, b: &PlannedDirectory) -> Result<bool> {
    Ok(within(a, b, directory_id)? || within(b, a, directory_id)?)
}

fn within(
    child: &PlannedDirectory,
    parent: &PlannedDirectory,
    identify: impl Fn(&Path) -> Result<Option<(u64, u64)>>,
) -> Result<bool> {
    let required =
        |p: &Path| identify(p)?.with_context(|| format!("directory disappeared: {}", p.display()));
    let parent_id = required(&parent.existing)?;
    let child_id = required(&child.existing)?;
    for ancestor in child.existing.ancestors() {
        if required(ancestor)? == parent_id {
            let below = child.existing.strip_prefix(ancestor)?;
            // Bind views of the same ancestor may have different overmounts.
            // Compare the actual endpoint in both views before comparing the
            // as-yet-uncreated suffix; the spelling alone proves nothing.
            if below.join(&child.missing).starts_with(&parent.missing)
                && identify(&parent.existing.join(below))? == Some(child_id)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_lookup_distinguishes_absence_from_unresolvable_paths() {
        let fixture = tempfile::tempdir().unwrap();
        let regular = fixture.path().join("regular");
        std::fs::write(&regular, "").unwrap();
        assert!(directory_id(fixture.path()).unwrap().is_some());
        assert_eq!(directory_id(&fixture.path().join("missing")).unwrap(), None);
        assert_eq!(directory_id(&regular).unwrap(), None);
        assert_eq!(directory_id(&regular.join("child")).unwrap(), None);

        let recursive_link = fixture.path().join("loop");
        std::os::unix::fs::symlink("loop", &recursive_link).unwrap();
        let error = directory_id(&recursive_link).unwrap_err();
        assert!(error.downcast_ref::<std::io::Error>().is_some());
        assert!(error.to_string().contains("inspect directory"));
    }

    #[test]
    fn alias_comparison_checks_the_endpoint_in_both_mount_views() {
        let parent = PlannedDirectory {
            existing: "/source".into(),
            missing: "".into(),
        };
        let child = PlannedDirectory {
            existing: "/view/skills".into(),
            missing: "new".into(),
        };
        for (projected, overlaps) in [(Some((1, 3)), true), (Some((2, 3)), false), (None, false)] {
            let identify = |p: &Path| {
                Ok(match p.to_str().unwrap() {
                    "/" => Some((1, 1)),
                    "/source" | "/view" => Some((1, 2)),
                    "/view/skills" => Some((1, 3)),
                    "/source/skills" => projected,
                    _ => None,
                })
            };
            assert_eq!(within(&child, &parent, identify).unwrap(), overlaps);
        }
        let missing = |existing: &str, suffix: &str| PlannedDirectory {
            existing: existing.into(),
            missing: suffix.into(),
        };
        let alias = |p: &Path| Ok(Some((1, if p == Path::new("/") { 1 } else { 2 })));
        assert!(
            within(
                &missing("/view", "new/child"),
                &missing("/source", "new"),
                alias
            )
            .unwrap()
        );
        assert!(!within(&missing("/view", "news"), &missing("/source", "new"), alias).unwrap());
    }
}
