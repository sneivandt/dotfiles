//! Runtime completion candidates and shell registration scripts.

use std::ffi::OsString;
use std::process::Command as ProcessCommand;

use clap_complete::CompletionCandidate;
use serde::Deserialize;

const COMPLETION_ENV: &str = "DOTFILES_COMPLETE";

const POWERSHELL_DOT_COMPLETER: &str = r"
Register-ArgumentCompleter -CommandName 'dot' -ParameterName 'Arguments' -ScriptBlock {
    param($commandName, $parameterName, $wordToComplete, $commandAst, $fakeBoundParameters)

    $expandedLine = [regex]::Replace(
        $commandAst.ToString(),
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

/// Complete task selectors from the same read-only discovery path as `dotfiles tasks`.
pub fn task_candidates() -> Vec<CompletionCandidate> {
    let words = completion_words(std::env::args_os().collect());
    let memberships = task_memberships(&words);
    let repository_args = repository_args(&words);
    let Ok(executable) = std::env::current_exe() else {
        return Vec::new();
    };
    let Ok(output) = ProcessCommand::new(executable)
        .args(["tasks", "--format", "json"])
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
    let command = words.iter().find_map(|word| match word.to_str()? {
        "install" => Some("install"),
        "update" => Some("update"),
        "uninstall" => Some("uninstall"),
        "check" => Some("check"),
        _ => None,
    });
    match command {
        Some("install") if words.iter().any(|word| word == "--update") => &["update"],
        Some("install") => &["install"],
        Some("update") => &["update"],
        Some("uninstall") => &["uninstall"],
        Some("check") => &["check"],
        Some(_) | None => &[],
    }
}

fn repository_args(words: &[OsString]) -> Vec<OsString> {
    let mut result = Vec::new();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let Some(text) = word.to_str() else {
            index = index.saturating_add(1);
            continue;
        };
        if matches!(text, "--profile" | "-p" | "--root" | "--overlay") {
            if let Some(value) = words.get(index.saturating_add(1)) {
                result.push(word.clone());
                result.push(value.clone());
                index = index.saturating_add(2);
                continue;
            }
        } else if text.starts_with("--profile=")
            || text.starts_with("--root=")
            || text.starts_with("--overlay=")
            || (text.starts_with("-p") && text.len() > 2)
        {
            result.push(word.clone());
        }
        index = index.saturating_add(1);
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

    #[test]
    fn profile_candidates_have_descriptions() {
        let profiles = profile_candidates();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].get_value(), "base");
        assert!(profiles[0].get_help().is_some());
    }

    #[test]
    fn repository_options_are_forwarded_to_task_discovery() {
        let words = [
            "dotfiles",
            "install",
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
    fn update_modes_complete_normal_and_update_only_tasks() {
        for words in [
            vec!["dotfiles", "update"],
            vec!["dotfiles", "install", "--update"],
        ] {
            let words = words.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert_eq!(task_memberships(&words), ["update"]);
        }
    }

    #[test]
    fn uninstall_completes_uninstall_tasks() {
        let words = ["dotfiles", "uninstall"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(task_memberships(&words), ["uninstall"]);
    }

    #[test]
    fn task_candidates_follow_command_membership() {
        let json = br#"[
            {"selector":"symlinks","task":"Home symlinks","commands":["install","update","uninstall"]},
            {"selector":"shellcheck","task":"Shellcheck","commands":["check"]},
            {"selector":"pin-only","task":"Pin updater","commands":["update"]}
        ]"#;

        let install = task_candidates_from_json(json, &["install"]);
        assert_eq!(install.len(), 1);
        assert_eq!(install[0].get_value(), "symlinks");
        assert_eq!(
            install[0].get_help().map(ToString::to_string).as_deref(),
            Some("Home symlinks")
        );

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
        assert!(powershell.contains("-CommandName 'dot' -ParameterName 'Arguments'"));
    }
}
