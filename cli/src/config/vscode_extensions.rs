use super::define_flat_config;

define_flat_config! {
    /// A VS Code extension to install.
    VsCodeExtension {
        /// Extension identifier in `publisher.name` format (e.g., `"github.copilot-chat"`).
        id
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::ini;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn load_desktop_extensions() {
        let (_dir, path) = write_temp_ini("[desktop]\ngithub.copilot-chat\nms-python.python\n");
        let extensions: Vec<VsCodeExtension> =
            ini::load_flat(&path, &["base".to_string(), "desktop".to_string()]).unwrap();
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].id, "github.copilot-chat");
    }

    #[test]
    fn inactive_category_excluded() {
        let (_dir, path) =
            write_temp_ini("[base]\ngithub.copilot\n\n[desktop]\ngithub.copilot-chat\n");
        let extensions: Vec<VsCodeExtension> =
            ini::load_flat(&path, &["base".to_string()]).unwrap();
        assert_eq!(extensions.len(), 1, "desktop section should not be loaded");
        assert_eq!(extensions[0].id, "github.copilot");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(ini::load_flat::<VsCodeExtension>);
    }
}
