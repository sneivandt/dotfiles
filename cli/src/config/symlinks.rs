//! Symlink configuration loading.
use super::define_flat_config;

define_flat_config! {
    /// A symlink to create: source (in symlinks/) → target (in $HOME).
    Symlink {
        /// Relative path under symlinks/ directory.
        source
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::ini;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn load_base_symlinks() {
        let (_dir, path) =
            write_temp_ini("[base]\nbashrc\nconfig/git/config\n\n[desktop]\nconfig/i3\n");
        let symlinks: Vec<Symlink> = ini::load_flat(&path, &["base".to_string()]).unwrap();
        assert_eq!(symlinks.len(), 2);
        assert_eq!(symlinks[0].source, "bashrc");
        assert_eq!(symlinks[1].source, "config/git/config");
    }

    #[test]
    fn load_multi_category() {
        let (_dir, path) = write_temp_ini("[base]\nbashrc\n\n[arch,desktop]\nconfig/i3\n");
        let symlinks: Vec<Symlink> = ini::load_flat(
            &path,
            &[
                "base".to_string(),
                "arch".to_string(),
                "desktop".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(symlinks.len(), 2);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(ini::load_flat::<Symlink>);
    }
}
