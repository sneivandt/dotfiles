use anyhow::Result;

use super::{Context, Task, TaskResult};

/// Check that configured fonts are present on the system.
pub struct CheckFonts;

impl Task for CheckFonts {
    fn name(&self) -> &str {
        "Check fonts"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        !ctx.config.fonts.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let font_list = get_installed_fonts();
        let mut found = 0u32;
        let mut missing = 0u32;

        for font in &ctx.config.fonts {
            if font_list
                .as_ref()
                .is_some_and(|list| list.contains(&font.name))
            {
                found += 1;
                ctx.log.debug(&format!("ok: {} (installed)", font.name));
            } else {
                missing += 1;
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("missing font: {}", font.name));
                } else {
                    ctx.log.warn(&format!("font missing: {}", font.name));
                }
            }
        }

        if ctx.dry_run {
            ctx.log
                .info(&format!("{found} installed, {missing} missing"));
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .info(&format!("{found} fonts found, {missing} missing"));
        Ok(TaskResult::Ok)
    }
}

/// Query installed fonts once and return the full list.
fn get_installed_fonts() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        crate::exec::run_unchecked("fc-list", &[])
            .ok()
            .map(|r| r.stdout)
    }

    #[cfg(target_os = "windows")]
    {
        let fonts_dir = std::env::var("WINDIR")
            .map(|w| std::path::PathBuf::from(w).join("Fonts"))
            .unwrap_or_default();
        if fonts_dir.exists() {
            std::fs::read_dir(&fonts_dir).ok().map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        } else {
            None
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}
