use anyhow::Result;

use crate::cli::{GlobalOpts, InstallOpts};
use crate::logging::Logger;
use crate::tasks::{self, Context, Task};

/// Run the install command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(global: &GlobalOpts, opts: &InstallOpts, log: &Logger) -> Result<()> {
    let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    log.info(&format!("dotfiles {version}"));

    let setup = super::CommandSetup::init(global, log)?;
    log.info(&format!(
        "loaded {} packages, {} symlinks",
        setup.config.packages.len(),
        setup.config.symlinks.len()
    ));

    let ctx = Context::new(&setup.config, &setup.platform, log, global.dry_run)?;

    // Build the task list
    let all_tasks: Vec<Box<dyn Task>> = vec![
        Box::new(tasks::developer_mode::EnableDeveloperMode),
        Box::new(tasks::sparse_checkout::SparseCheckout),
        Box::new(tasks::update::UpdateRepository),
        Box::new(tasks::hooks::GitHooks),
        Box::new(tasks::git_config::ConfigureGit),
        Box::new(tasks::packages::InstallPackages),
        Box::new(tasks::packages::InstallParu),
        Box::new(tasks::packages::InstallAurPackages),
        Box::new(tasks::symlinks::InstallSymlinks),
        Box::new(tasks::chmod::ApplyFilePermissions),
        Box::new(tasks::shell::ConfigureShell),
        Box::new(tasks::vscode::InstallVsCodeExtensions),
        Box::new(tasks::copilot_skills::InstallCopilotSkills),
        Box::new(tasks::systemd::ConfigureSystemd),
        Box::new(tasks::registry::ApplyRegistry),
    ];

    // Filter by --skip and --only
    let tasks_to_run: Vec<&dyn Task> = all_tasks
        .iter()
        .filter(|t| {
            let name = t.name().to_lowercase();
            if !opts.only.is_empty() {
                return opts.only.iter().any(|o| name.contains(&o.to_lowercase()));
            }
            if !opts.skip.is_empty() {
                return !opts.skip.iter().any(|s| name.contains(&s.to_lowercase()));
            }
            true
        })
        .map(std::convert::AsRef::as_ref)
        .collect();

    super::run_tasks_to_completion(&tasks_to_run, &ctx, log)
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub fn resolve_root(global: &GlobalOpts) -> Result<std::path::PathBuf> {
    if let Some(ref root) = global.root {
        return Ok(root.clone());
    }

    // Auto-detect: binary location, DOTFILES_ROOT env, or current dir
    if let Ok(root) = std::env::var("DOTFILES_ROOT") {
        return Ok(std::path::PathBuf::from(root));
    }

    // Try to find the repository root from the current binary's location
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        // Check if we're in cli/target/release/ or bin/
        let candidates = [
            parent.join("../../.."), // cli/target/release/ → repo root
            parent.join(".."),       // bin/ → repo root
        ];
        for candidate in &candidates {
            if candidate.join("conf").exists() && candidate.join("symlinks").exists() {
                return Ok(std::fs::canonicalize(candidate)?);
            }
        }
    }

    // Last resort: current directory
    let cwd = std::env::current_dir()?;
    if cwd.join("conf").exists() {
        return Ok(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}
