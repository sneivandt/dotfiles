//! Runtime completion candidates and shell registration scripts.

use std::ffi::OsString;
use std::process::Command as ProcessCommand;

use clap_complete::CompletionCandidate;
use serde::Deserialize;

/// Completion uses the same command context as argument parsing, including
/// options before an explicit command and the implicit update command.
pub(super) fn command() -> clap::Command {
    let words = completion_words(std::env::args_os().collect());
    command_for_words(&words)
}

fn command_for_words(words: &[OsString]) -> clap::Command {
    use clap::CommandFactory as _;
    let mut canonical = super::cli::Cli::command();
    canonical.build();
    let mut normalized = words.to_vec();
    super::cli::normalize_command_position(&mut normalized);
    let explicit = normalized.get(1).and_then(|word| word.to_str());
    let Some(selected) = canonical
        .find_subcommand(explicit.unwrap_or("update"))
        .or_else(|| canonical.find_subcommand("update"))
    else {
        return canonical;
    };
    let present = present_arguments(selected, normalized.get(2..).unwrap_or_default());
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
                && option_value(&normalized, "--graph") != Some("update");
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
    canonical
        .mut_subcommands(|subcommand| {
            let name = match subcommand.get_name() {
                "update" => "update",
                "remove" => "remove",
                "check" => "check",
                "list" => "list",
                "log" => "log",
                "completions" => "completions",
                "help" => "help",
                _ => return subcommand,
            };
            context
                .clone()
                .name(name)
                .about(subcommand.get_about().cloned().unwrap_or_default())
                .hide(subcommand.is_hide_set())
        })
        .args(options)
}

/// Collect option identities, respecting value boundaries and short clusters.
/// Probe with valid placeholder values so incomplete input cannot mask conflicts.
fn present_arguments(command: &clap::Command, words: &[OsString]) -> Vec<clap::Id> {
    let mut present = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let Some(text) = word.to_str() else { continue };
        if text == "--" {
            break;
        }
        if let Some(long) = text.strip_prefix("--") {
            let (long, attached) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            if let Some(arg) = command
                .get_arguments()
                .find(|arg| arg.get_long() == Some(long))
            {
                present.push(arg.get_id().clone());
                if arg.get_action().takes_values() && !attached {
                    let _ = words.next();
                }
            }
        } else if let Some(shorts) = text.strip_prefix('-') {
            let mut shorts = shorts.chars();
            while let Some(short) = shorts.next() {
                if let Some(arg) = command
                    .get_arguments()
                    .find(|arg| arg.get_short() == Some(short))
                {
                    present.push(arg.get_id().clone());
                    if arg.get_action().takes_values() {
                        if shorts.as_str().is_empty() {
                            let _ = words.next();
                        }
                        break;
                    }
                }
            }
        } else if !text.is_empty()
            && let Some(arg) = command.get_positionals().next()
        {
            present.push(arg.get_id().clone());
        }
    }
    present
}

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

#[derive(Deserialize)]
struct TaskListing {
    selector: String,
    task: String,
    commands: Vec<String>,
}

/// Name of the environment variable that activates runtime completion.
pub const fn environment_variable() -> &'static str {
    COMPLETION_ENV
}

/// Return the shell code that registers runtime completion for `dotfiles`.
pub fn registration(shell: clap_complete::Shell) -> String {
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
pub fn profile_candidates() -> Vec<CompletionCandidate> {
    crate::app::config::profiles::available()
        .iter()
        .map(|profile| {
            CompletionCandidate::new(profile.name).help(Some(profile.description.into()))
        })
        .collect()
}

/// Complete task selectors from the same read-only discovery path as `dotfiles list`.
pub fn task_candidates() -> Vec<CompletionCandidate> {
    let words = completion_words(std::env::args_os().collect());
    let memberships = task_memberships(&words);
    let repository_args = repository_args(&words);
    let Ok(executable) = std::env::current_exe() else {
        return Vec::new();
    };
    let Ok(output) = ProcessCommand::new(executable)
        .args(["list", "--all", "--format", "json"])
        .args(repository_args)
        .env_remove(COMPLETION_ENV)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    task_candidates_from_json(&output.stdout, memberships)
}

fn completion_words(args: Vec<OsString>) -> Vec<OsString> {
    args.into_iter()
        .skip_while(|arg| arg != "--")
        .skip(1)
        .collect()
}

fn task_memberships(words: &[OsString]) -> &'static [&'static str] {
    let mut normalized = words.to_vec();
    super::cli::normalize_command_position(&mut normalized);
    match normalized.get(1).and_then(|word| word.to_str()) {
        Some("update") | None => &["update"],
        Some("remove") => &["remove"],
        Some("check") => &["check"],
        Some("list") => match option_value(&normalized, "--graph") {
            Some("update") => &["update"],
            Some("remove") => &["remove"],
            Some("check") => &["check"],
            _ => &[],
        },
        Some(word) if word.starts_with('-') => &["update"],
        Some(_) => &[],
    }
}

fn option_value<'a>(words: &'a [OsString], option: &str) -> Option<&'a str> {
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let text = word.to_str()?;
        if text == option {
            return words.next()?.to_str();
        }
        if let Some((name, value)) = text.split_once('=')
            && name == option
        {
            return Some(value);
        }
    }
    None
}

fn repository_args(words: &[OsString]) -> Vec<OsString> {
    let mut result = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        let Some(text) = word.to_str() else {
            continue;
        };
        if matches!(text, "--profile" | "-p" | "--root" | "--overlay") {
            if let Some(value) = words.next() {
                result.extend([word.clone(), value.clone()]);
            }
        } else if text.starts_with("--profile=")
            || text.starts_with("--root=")
            || text.starts_with("--overlay=")
            || (text.starts_with("-p") && text.len() > 2)
        {
            result.push(word.clone());
        }
    }
    result
}

fn task_candidates_from_json(json: &[u8], memberships: &[&str]) -> Vec<CompletionCandidate> {
    let Ok(listings) = serde_json::from_slice::<Vec<TaskListing>>(json) else {
        return Vec::new();
    };
    listings
        .into_iter()
        .filter(|listing| {
            memberships.is_empty()
                || listing
                    .commands
                    .iter()
                    .any(|command| memberships.contains(&command.as_str()))
        })
        .map(|listing| CompletionCandidate::new(listing.selector).help(Some(listing.task.into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(words: &[&str], index: usize) -> Vec<String> {
        let words = words.iter().map(OsString::from).collect::<Vec<_>>();
        let mut command = command_for_words(&words);
        clap_complete::engine::complete(&mut command, words, index, None)
            .expect("complete CLI arguments")
            .iter()
            .filter(|candidate| !candidate.is_hide_set())
            .map(|candidate| candidate.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn format_candidates_follow_the_command_before_and_after_options() {
        for (command, expected) in [
            ("update", vec!["human", "jsonl"]),
            ("remove", vec!["human", "jsonl"]),
            ("check", vec!["human", "jsonl"]),
            ("list", vec!["table", "plain", "json"]),
        ] {
            for (words, index) in [
                (vec!["dotfiles", command, "--format", ""], 3),
                (vec!["dotfiles", "--format", "", command], 2),
            ] {
                assert_eq!(complete(&words, index), expected, "{words:?}");
                for value in &expected {
                    super::super::cli::Cli::try_parse_from([
                        "dotfiles", "--format", value, command,
                    ])
                    .expect("every suggested format must parse");
                }
            }
        }
        assert_eq!(
            complete(&["dotfiles", "--format", ""], 2),
            ["human", "jsonl"]
        );
        assert_eq!(
            complete(&["dotfiles", "--format=", "list"], 1),
            ["--format=table", "--format=plain", "--format=json"]
        );
        assert_eq!(
            complete(&["dotfiles", "--list", "--format", "", "log"], 3),
            ["table", "plain", "json"]
        );
    }

    fn assert_flags(words: &[&str], forbidden: &[&str], allowed: &str) {
        let candidates = complete(words, words.len() - 1);
        for flag in forbidden {
            assert!(
                !candidates.iter().any(|candidate| candidate == flag),
                "{words:?}: {flag}"
            );
        }
        assert!(
            candidates.iter().any(|candidate| candidate == allowed),
            "{words:?}: {allowed}"
        );
    }

    #[test]
    fn completion_respects_conflicts_and_required_flags() {
        for (words, forbidden, allowed) in [
            (
                vec!["dotfiles", "update", "--quiet", "--"],
                vec!["--verbose", "--format"],
                "--dry-run",
            ),
            (
                vec!["dotfiles", "log", "--list", "--"],
                vec!["--raw", "--task", "--id", "--verbose"],
                "--format",
            ),
            (vec!["dotfiles", "log", "--"], vec!["--format"], "--list"),
            (
                vec!["dotfiles", "list", "--"],
                vec!["--only", "--skip", "--with-deps"],
                "--graph",
            ),
            (
                vec!["dotfiles", "list", "--graph", "check", "--"],
                vec!["--all"],
                "--only",
            ),
            (
                vec!["dotfiles", "update", "--"],
                vec!["--with-deps"],
                "--only",
            ),
            (
                vec!["dotfiles", "update", "--only", "symlinks", "--"],
                vec![],
                "--with-deps",
            ),
        ] {
            assert_flags(&words, &forbidden, allowed);
        }
        assert!(complete(&["dotfiles", "log", "--format", ""], 3).is_empty());
        assert!(complete(&["dotfiles", "update", "--quiet", "--format", ""], 4).is_empty());
    }

    #[test]
    fn completion_handles_repeated_options_short_flags_and_positionals() {
        for (words, forbidden, allowed) in [
            (
                vec!["dotfiles", "log", "0", "--"],
                vec!["--list", "--id"],
                "--raw",
            ),
            (
                vec!["dotfiles", "update", "-q", "--"],
                vec!["--format", "--verbose", "--quiet"],
                "--dry-run",
            ),
            (
                vec!["dotfiles", "update", "--profile=base", "-q", "--"],
                vec!["--format", "--verbose"],
                "--dry-run",
            ),
            (
                vec!["dotfiles", "update", "--profile", "base", "--"],
                vec!["--profile"],
                "--only",
            ),
        ] {
            assert_flags(&words, &forbidden, allowed);
        }
    }

    #[test]
    fn graph_dependency_completion_requires_an_update_graph() {
        for (words, forbidden, allowed) in [
            (
                vec![
                    "dotfiles",
                    "list",
                    "--graph",
                    "check",
                    "--only",
                    "shellcheck",
                    "--",
                ],
                vec!["--with-deps"],
                "--skip",
            ),
            (
                vec![
                    "dotfiles",
                    "list",
                    "--graph=remove",
                    "--only",
                    "symlinks",
                    "--",
                ],
                vec!["--with-deps"],
                "--skip",
            ),
            (
                vec![
                    "dotfiles", "list", "--graph", "update", "--only", "symlinks", "--",
                ],
                vec![],
                "--with-deps",
            ),
        ] {
            assert_flags(&words, &forbidden, allowed);
        }
        assert_eq!(
            complete(
                &[
                    "dotfiles",
                    "list",
                    "--only",
                    "symlinks",
                    "--with-deps",
                    "--graph",
                    ""
                ],
                6
            ),
            ["update"]
        );
        assert_eq!(
            complete(&["dotfiles", "list", "--graph", ""], 3),
            ["update", "remove", "check"]
        );
        assert_eq!(
            complete(
                &[
                    "dotfiles",
                    "list",
                    "--graph",
                    "update",
                    "--only",
                    "symlinks",
                    "--with-deps",
                    "--format",
                    ""
                ],
                8
            ),
            ["table", "plain", "json"]
        );
        assert_eq!(
            complete(
                &[
                    "dotfiles",
                    "list",
                    "--graph",
                    "update",
                    "--only",
                    "symlinks",
                    "--with-deps",
                    "--profile",
                    ""
                ],
                8
            ),
            ["base", "desktop"]
        );
    }

    #[test]
    fn command_names_used_as_values_do_not_change_completion_context() {
        for name in [
            "log",
            "list",
            "check",
            "remove",
            "update",
            "completions",
            "help",
        ] {
            assert_eq!(
                complete(&["dotfiles", "--root", name, "--format", ""], 4),
                ["human", "jsonl"],
                "root named {name}"
            );
            let candidates = complete(&["dotfiles", "--root", name, "--quiet", "--"], 4);
            assert!(
                !candidates.contains(&"--format".to_owned()),
                "root named {name}"
            );
            assert!(
                !candidates.contains(&"--quiet".to_owned()),
                "repeated quiet, root named {name}"
            );
            assert!(
                candidates.contains(&"--dry-run".to_owned()),
                "root named {name}"
            );
        }
        assert_eq!(
            complete(&["dotfiles", "--root", "log", "list", "--format", ""], 5),
            ["table", "plain", "json"]
        );
        let candidates = complete(&["dotfiles", "log", "--command", "check", "--"], 4);
        assert!(candidates.contains(&"--list".to_owned()));
        assert!(!candidates.contains(&"--profile".to_owned()));
    }

    #[test]
    fn graph_selector_membership_matches_the_selected_graph() {
        for command in ["update", "remove", "check"] {
            for graph in [vec!["--graph", command], vec![]] {
                let mut words = vec!["dotfiles", "list"];
                if graph.is_empty() {
                    let attached = format!("--graph={command}");
                    let attached_words =
                        ["dotfiles", "list", &attached, "--only", ""].map(OsString::from);
                    assert_eq!(task_memberships(&attached_words), [command]);
                } else {
                    words.extend(graph);
                    words.extend(["--skip", ""]);
                    assert_eq!(
                        task_memberships(&words.iter().map(OsString::from).collect::<Vec<_>>()),
                        [command]
                    );
                }
            }
        }
    }

    #[test]
    fn prefix_options_do_not_change_task_completion_membership() {
        for (words, expected) in [
            (
                vec!["dotfiles", "--profile", "base", "check", "--only", ""],
                &["check"][..],
            ),
            (
                vec!["dotfiles", "--root=update", "remove", "--only", ""],
                &["remove"][..],
            ),
            (
                vec!["dotfiles", "--profile", "check", "--only", ""],
                &["update"][..],
            ),
            (
                vec!["dotfiles", "-vpbase", "check", "--only", ""],
                &["check"][..],
            ),
        ] {
            let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert_eq!(task_memberships(&words), expected);
        }
    }

    #[test]
    fn default_update_options_are_available_to_completion() {
        let mut command = command();
        command.build();
        for id in ["profile", "only", "dry_run", "no_repo_update"] {
            assert!(command.get_arguments().any(|arg| arg.get_id() == id));
        }
    }

    #[test]
    fn profile_candidates_have_descriptions() {
        let profiles = profile_candidates();
        assert_eq!(
            profiles
                .iter()
                .map(CompletionCandidate::get_value)
                .collect::<Vec<_>>(),
            ["base", "desktop"]
        );
        assert!(profiles.iter().all(|profile| {
            profile
                .get_help()
                .is_some_and(|help| !help.to_string().is_empty())
        }));
    }

    #[test]
    fn repository_options_are_forwarded_to_task_discovery() {
        let words = [
            "dotfiles",
            "update",
            "--profile=desktop",
            "--root",
            "/repo",
            "-pbase",
            "--overlay=/private",
            "--only",
            "sys",
        ]
        .map(OsString::from);

        assert_eq!(
            repository_args(&words),
            [
                "--profile=desktop",
                "--root",
                "/repo",
                "-pbase",
                "--overlay=/private"
            ]
            .map(OsString::from)
        );
    }

    #[test]
    fn repository_options_preserve_value_boundaries() {
        for (words, expected) in [
            (vec![], vec![]),
            (vec!["--root"], vec![]),
            (vec!["-p", "base", "--root"], vec!["-p", "base"]),
            (vec!["--root", "--overlay"], vec!["--root", "--overlay"]),
            (
                vec!["--overlay", "", "--root=/repo", "--root", "/other"],
                vec!["--overlay", "", "--root=/repo", "--root", "/other"],
            ),
        ] {
            let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
            let expected = expected.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert_eq!(repository_args(&words), expected, "{words:?}");
        }
    }

    #[test]
    fn update_modes_complete_normal_and_update_only_tasks() {
        for words in [vec!["dotfiles", "update"], vec!["dotfiles", "--dry-run"]] {
            let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert_eq!(task_memberships(&words), ["update"]);
        }
    }

    #[test]
    fn uninstall_completes_uninstall_tasks() {
        let words = ["dotfiles", "remove"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(task_memberships(&words), ["remove"]);
    }

    #[test]
    fn standalone_and_unknown_commands_have_no_execution_task_membership() {
        for command in ["list", "log", "completions", "help", "unknown"] {
            let words = ["dotfiles", command].map(OsString::from);
            let expected: &[&str] = &[];
            assert_eq!(task_memberships(&words), expected, "{command}");
        }
    }

    #[test]
    fn task_candidates_follow_command_membership() {
        let json = br#"[
            {"selector":"symlinks","task":"Home symlinks","commands":["update","remove"]},
            {"selector":"shellcheck","task":"Shellcheck","commands":["check"]},
            {"selector":"pin-only","task":"Pin updater","commands":["update"]}
        ]"#;

        let update = task_candidates_from_json(json, &["update"]);
        assert_eq!(update.len(), 2);
        assert_eq!(update[0].get_value(), "symlinks");
        assert_eq!(update[1].get_value(), "pin-only");

        let check = task_candidates_from_json(json, &["check"]);
        assert_eq!(check.len(), 1);
        assert_eq!(check[0].get_value(), "shellcheck");
    }

    #[test]
    fn registration_uses_runtime_completion_and_keeps_the_dot_completer() {
        let zsh = registration(clap_complete::Shell::Zsh);
        assert_eq!(zsh, "source <(DOTFILES_COMPLETE=zsh dotfiles)\n");

        let powershell = registration(clap_complete::Shell::PowerShell);
        assert!(powershell.contains("DOTFILES_COMPLETE = 'powershell'"));
        assert!(powershell.contains("-Native -CommandName 'dot'"));
        assert!(!powershell.contains("-ParameterName 'Arguments'"));
        assert!(powershell.contains("$cursorPosition - $commandAst.Extent.StartOffset"));
    }
}
