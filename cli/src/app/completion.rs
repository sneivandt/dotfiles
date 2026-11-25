//! Runtime completion candidates and shell registration scripts.

use std::ffi::{OsStr, OsString};

use clap_complete::CompletionCandidate;
use clap_lex::RawArgs;

use super::cli_words::{self, Word};
use super::commands::list::TaskListing;

static CANONICAL_CLI: std::sync::OnceLock<clap::Command> = std::sync::OnceLock::new();

/// Completion uses the same command context as argument parsing, including
/// options before an explicit command and the implicit update command.
pub(super) fn command() -> clap::Command {
    let words = completion_words(std::env::args_os().collect());
    command_for_words(&words)
}

fn command_for_words(words: &[OsString]) -> clap::Command {
    use clap::CommandFactory as _;
    let mut canonical = super::cli::Cli::command();
    let mut normalized = words.to_vec();
    super::cli::normalize_command_position(&mut normalized, &canonical);
    canonical.build();
    let explicit = normalized.get(1).and_then(|word| word.to_str());
    let Some(selected) = canonical
        .find_subcommand(explicit.unwrap_or("update"))
        .or_else(|| canonical.find_subcommand("update"))
    else {
        return canonical;
    };
    let raw = RawArgs::new(normalized.iter().skip(2));
    let present = present_arguments(selected, &raw);
    let options = selected
        .get_arguments()
        .cloned()
        .map(|arg| {
            let arg = if selected.get_name() == "list"
                && arg.get_id() == "graph"
                && present.iter().any(|id| id == "with_deps")
            {
                arg.add(clap_complete::ArgValueCandidates::new(|| {
                    vec![CompletionCandidate::new("update")]
                }))
            } else {
                arg
            };
            let graph_expansion_unavailable = selected.get_name() == "list"
                && arg.get_id() == "with_deps"
                && graph_value(&raw, &canonical) != Some("update");
            if !graph_expansion_unavailable && argument_available(selected, &present, &arg) {
                if present.contains(arg.get_id())
                    && !matches!(arg.get_action(), clap::ArgAction::Append)
                {
                    arg.hide(true)
                } else {
                    arg
                }
            } else {
                // Hiding an option alone does not suppress its value candidates.
                arg.hide(true)
                    .add(clap_complete::ArgValueCandidates::new(Vec::new))
            }
        })
        .collect::<Vec<_>>();
    let context = options.iter().fold(selected.clone(), |command, arg| {
        command.mut_arg(arg.get_id(), |_| arg.clone())
    });
    // Every traversal uses the selected command's syntax. The completion
    // engine otherwise treats command names used as option values (for example
    // --root log) as changes of context.
    for name in CANONICAL_CLI
        .get_or_init(super::cli::Cli::command)
        .get_subcommands()
        .map(clap::Command::get_name)
        .chain(std::iter::once("help"))
    {
        canonical = canonical.mut_subcommand(name, |subcommand| {
            context
                .clone()
                .name(name)
                .about(subcommand.get_about().cloned().unwrap_or_default())
                .hide(subcommand.is_hide_set())
        });
    }
    canonical.args(options)
}

/// Collect option identities, respecting value boundaries and short clusters.
fn present_arguments(command: &clap::Command, raw: &RawArgs) -> Vec<clap::Id> {
    cli_words::words(raw, command.get_arguments().collect())
        .filter_map(|word| match word {
            Word::Option(arg, _) => Some(arg),
            Word::Positional(_, value) if !value.is_empty() => command.get_positionals().next(),
            Word::Positional(_, _) => None,
        })
        .map(|arg| arg.get_id().clone())
        .collect()
}

/// Probe with valid placeholder values so incomplete input cannot mask conflicts.
fn argument_available(
    command: &clap::Command,
    present: &[clap::Id],
    candidate: &clap::Arg,
) -> bool {
    use clap::error::ErrorKind;
    let mut args = vec![OsString::from(command.get_name())];
    for arg in command
        .get_arguments()
        .filter(|arg| present.contains(arg.get_id()))
        .chain((!present.contains(candidate.get_id())).then_some(candidate))
    {
        if let Some(long) = arg.get_long() {
            args.push(format!("--{long}").into());
        } else if let Some(short) = arg.get_short() {
            args.push(format!("-{short}").into());
        }
        if arg.get_action().takes_values() {
            let value = arg
                .get_value_parser()
                .possible_values()
                .and_then(|mut values| values.next().map(|value| value.get_name().to_owned()))
                .unwrap_or_else(|| "0".to_owned());
            args.push(value.into());
        }
    }
    match command.clone().try_get_matches_from(args) {
        Ok(_) => true,
        Err(error) => !matches!(
            error.kind(),
            ErrorKind::ArgumentConflict | ErrorKind::MissingRequiredArgument
        ),
    }
}

const COMPLETION_ENV: &str = "DOTFILES_COMPLETE";

const POWERSHELL_DOT_COMPLETER: &str = r"
Register-ArgumentCompleter -Native -CommandName 'dot' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $relativeCursor = $cursorPosition - $commandAst.Extent.StartOffset
    $commandLine = $commandAst.ToString().PadRight($relativeCursor).Substring(0, $relativeCursor)
    $expandedLine = [regex]::Replace(
        $commandLine,
        '^dot(?=\s|$)',
        'dotfiles',
        1
    )
    (TabExpansion2 -InputScript $expandedLine -CursorColumn $expandedLine.Length).CompletionMatches
}
";

/// Name of the environment variable that activates runtime completion.
pub(crate) const fn environment_variable() -> &'static str {
    COMPLETION_ENV
}

/// Return the shell code that registers runtime completion for `dotfiles`.
pub(crate) fn registration(shell: clap_complete::Shell) -> String {
    match shell {
        clap_complete::Shell::Bash => "source <(DOTFILES_COMPLETE=bash dotfiles)\n".to_string(),
        clap_complete::Shell::Elvish => {
            "eval (E:DOTFILES_COMPLETE=elvish dotfiles | slurp)\n".to_string()
        }
        clap_complete::Shell::Fish => "DOTFILES_COMPLETE=fish dotfiles | source\n".to_string(),
        clap_complete::Shell::PowerShell => format!(
            r"$dotfilesPreviousComplete = $env:DOTFILES_COMPLETE
try {{
    $env:DOTFILES_COMPLETE = 'powershell'
    dotfiles | Out-String | Invoke-Expression
}}
finally {{
    if ($null -eq $dotfilesPreviousComplete) {{
        Remove-Item Env:\DOTFILES_COMPLETE -ErrorAction SilentlyContinue
    }}
    else {{
        $env:DOTFILES_COMPLETE = $dotfilesPreviousComplete
    }}
}}
{POWERSHELL_DOT_COMPLETER}"
        ),
        clap_complete::Shell::Zsh => "source <(DOTFILES_COMPLETE=zsh dotfiles)\n".to_string(),
        _ => String::new(),
    }
}

/// Complete the built-in profile names without changing profile selection.
pub(crate) fn profile_candidates() -> Vec<CompletionCandidate> {
    crate::app::config::profiles::available()
        .iter()
        .map(|profile| {
            CompletionCandidate::new(profile.name).help(Some(profile.description.into()))
        })
        .collect()
}

/// Complete task selectors from the same read-only discovery path as `dotfiles list`.
pub(crate) fn task_candidates() -> Vec<CompletionCandidate> {
    let words = completion_words(std::env::args_os().collect());
    let memberships = task_memberships(&words);
    let mut args = vec![
        OsString::from("dotfiles"),
        OsString::from("list"),
        OsString::from("--all"),
    ];
    args.extend(repository_args(&words));
    let Ok(super::cli::Cli {
        command: super::cli::Command::List(opts),
        ..
    }) = super::cli::Cli::try_parse_from(args)
    else {
        return Vec::new();
    };
    let Ok(listings) = super::commands::list::discover_all_listings(&opts.repository) else {
        return Vec::new();
    };
    task_candidates_from_listings(listings, memberships)
}

fn completion_words(args: Vec<OsString>) -> Vec<OsString> {
    args.into_iter()
        .skip_while(|arg| arg != "--")
        .skip(1)
        .collect()
}

fn task_memberships(words: &[OsString]) -> &'static [&'static str] {
    use clap::CommandFactory as _;
    let cli = super::cli::Cli::command();
    let mut normalized = words.to_vec();
    super::cli::normalize_command_position(&mut normalized, &cli);
    match normalized.get(1).and_then(|word| word.to_str()) {
        Some("update") | None => &["update"],
        Some("remove") => &["remove"],
        Some("check") => &["check"],
        Some("list") => match graph_value(&RawArgs::new(normalized.iter().skip(2)), &cli) {
            Some("update") => &["update"],
            Some("remove") => &["remove"],
            Some("check") => &["check"],
            _ => &[],
        },
        Some(_) => &[],
    }
}

fn graph_value<'a>(raw: &'a RawArgs, command: &'a clap::Command) -> Option<&'a str> {
    option_values(raw, command)
        .find(|(name, _)| *name == "graph")
        .and_then(|(_, value)| value.to_str())
}

fn repository_args(words: &[OsString]) -> Vec<OsString> {
    use clap::CommandFactory as _;
    option_values(&RawArgs::new(words), &super::cli::Cli::command())
        .filter(|(name, _)| matches!(*name, "profile" | "root" | "overlay"))
        .flat_map(|(name, value)| [format!("--{name}").into(), value.to_os_string()])
        .collect()
}

/// Read values with the same option boundaries as command normalization, even
/// while the current completion word is incomplete.
fn option_values<'a>(
    raw: &'a RawArgs,
    command: &'a clap::Command,
) -> impl Iterator<Item = (&'a str, &'a OsStr)> {
    cli_words::words(raw, cli_words::value_options(command)).filter_map(|word| match word {
        Word::Option(arg, Some(value)) => Some((arg.get_long()?, value)),
        Word::Positional(_, _) | Word::Option(_, None) => None,
    })
}

fn task_candidates_from_listings(
    listings: Vec<TaskListing>,
    memberships: &[&str],
) -> Vec<CompletionCandidate> {
    listings
        .into_iter()
        .filter(|listing| {
            memberships.is_empty()
                || listing
                    .commands
                    .iter()
                    .any(|command| memberships.contains(&command.label()))
        })
        .map(|listing| CompletionCandidate::new(listing.selector).help(Some(listing.task.into())))
        .collect()
}

#[cfg(test)]
#[path = "tests/completion.rs"]
mod tests;
