use super::define_flat_config;

define_flat_config! {
    /// A systemd user unit to enable.
    SystemdUnit {
        /// Unit name including extension (e.g., `"clean-home-tmp.timer"`).
        name
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::ini;
    use crate::config::test_helpers::{assert_load_missing_returns_empty, write_temp_ini};

    #[test]
    fn load_base_units() {
        let (_dir, path) =
            write_temp_ini("[base]\nclean-home-tmp.timer\n\n[arch,desktop]\ndunst.service\n");
        let units: Vec<SystemdUnit> = ini::load_flat(&path, &["base".to_string()]).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "clean-home-tmp.timer");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        assert_load_missing_returns_empty(ini::load_flat::<SystemdUnit>);
    }
}
