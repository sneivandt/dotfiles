use clap::{Parser, Subcommand};

/// Top-level CLI entry point for the dotfiles management engine.
#[derive(Parser, Debug)]
#[command(
    name = "dotfiles",
    about = "Cross-platform dotfiles management engine",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(flatten)]
    pub global: GlobalOpts,
}

/// Options shared across all subcommands.
#[derive(Parser, Debug, Clone)]
pub struct GlobalOpts {
    /// Profile to use (base, arch, desktop, arch-desktop, windows)
    #[arg(short, long, global = true)]
    pub profile: Option<String>,

    /// Preview changes without applying
    #[arg(short = 'd', long, global = true)]
    pub dry_run: bool,

    /// Override dotfiles root directory
    #[arg(long, global = true)]
    pub root: Option<std::path::PathBuf>,

    /// Disable parallel execution of operations within tasks (parallel is enabled by default)
    #[arg(long = "no-parallel", global = true, action = clap::ArgAction::SetFalse)]
    pub parallel: bool,
}

/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install dotfiles and configure system
    Install(InstallOpts),
    /// Remove installed dotfiles
    Uninstall(UninstallOpts),
    /// Run self-tests and validation
    Test(TestOpts),
    /// Print version information
    Version,
}

/// Options for the `install` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct InstallOpts {
    /// Skip specific tasks
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    /// Run only specific tasks
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
}

/// Options for the `uninstall` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct UninstallOpts {}

/// Options for the `test` subcommand.
#[derive(Parser, Debug, Clone)]
pub struct TestOpts {}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_install_with_profile() {
        let cli = Cli::parse_from(["dotfiles", "--profile", "arch", "install"]);
        assert_eq!(cli.global.profile, Some("arch".to_string()));
        assert!(matches!(cli.command, Command::Install(_)));
    }

    #[test]
    fn parse_install_with_profile_short() {
        let cli = Cli::parse_from(["dotfiles", "-p", "arch", "install"]);
        assert_eq!(cli.global.profile, Some("arch".to_string()));
        assert!(matches!(cli.command, Command::Install(_)));
    }

    #[test]
    fn parse_install_dry_run() {
        let cli = Cli::parse_from(["dotfiles", "--dry-run", "install"]);
        assert!(cli.global.dry_run);
    }

    #[test]
    fn parse_install_dry_run_short() {
        let cli = Cli::parse_from(["dotfiles", "-d", "install"]);
        assert!(cli.global.dry_run);
    }

    #[test]
    fn parse_install_skip_tasks() {
        let cli = Cli::parse_from(["dotfiles", "install", "--skip", "packages,fonts"]);
        assert!(
            matches!(&cli.command, Command::Install(_)),
            "Expected Install command"
        );
        if let Command::Install(opts) = cli.command {
            assert_eq!(opts.skip, vec!["packages", "fonts"]);
        }
    }

    #[test]
    fn parse_install_only_tasks() {
        let cli = Cli::parse_from(["dotfiles", "install", "--only", "symlinks"]);
        assert!(
            matches!(&cli.command, Command::Install(_)),
            "Expected Install command"
        );
        if let Command::Install(opts) = cli.command {
            assert_eq!(opts.only, vec!["symlinks"]);
        }
    }

    #[test]
    fn parse_version() {
        let cli = Cli::parse_from(["dotfiles", "version"]);
        assert!(matches!(cli.command, Command::Version));
    }

    #[test]
    fn parse_verbose() {
        let cli = Cli::parse_from(["dotfiles", "-v", "install"]);
        assert!(cli.verbose);
    }

    #[test]
    fn parse_uninstall() {
        let cli = Cli::parse_from(["dotfiles", "uninstall"]);
        assert!(matches!(cli.command, Command::Uninstall(_)));
    }

    #[test]
    fn parse_test() {
        let cli = Cli::parse_from(["dotfiles", "test"]);
        assert!(matches!(cli.command, Command::Test(_)));
    }

    #[test]
    fn parse_root_override() {
        let cli = Cli::parse_from(["dotfiles", "--root", "/tmp/dotfiles", "install"]);
        assert_eq!(
            cli.global.root,
            Some(std::path::PathBuf::from("/tmp/dotfiles"))
        );
    }

    #[test]
    fn parallel_is_enabled_by_default() {
        let cli = Cli::parse_from(["dotfiles", "install"]);
        assert!(cli.global.parallel, "parallel should be true by default");
    }

    #[test]
    fn no_parallel_disables_parallel() {
        let cli = Cli::parse_from(["dotfiles", "--no-parallel", "install"]);
        assert!(
            !cli.global.parallel,
            "--no-parallel should set parallel to false"
        );
    }
}
