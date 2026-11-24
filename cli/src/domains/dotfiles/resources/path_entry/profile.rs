//! Recognize the profile boundaries written by Dotfiles, not shell syntax.

use anyhow::{Result, ensure};

const MARKER: &str = "# Added by dotfiles";
const EXPORT: &str = r#"export PATH="$HOME/.local/bin:$PATH""#;
const LEGACY: &str = "# Added by dotfiles\nexport PATH=\"$HOME/.local/bin:$PATH\"\n";
const GUARD: &str = r#"# Added by dotfiles
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
"#;
const RETRY: &str = r#"then run: PATH=/usr/bin:/bin "$HOME/.local/bin/dotfiles" update --only path --no-repo-update"#;

fn only_comments(text: &str) -> bool {
    text.split('\n').all(|row| {
        let row = row.trim_start_matches([' ', '\t']);
        row.is_empty() || row.starts_with('#')
    })
}

fn check_unmanaged(text: &str) -> Result<()> {
    // Whitespace/CRLF lookalikes may be invalid shell or literal example text.
    // Recognize them for a diagnostic, never as permission to delete or replace.
    ensure!(
        !text
            .lines()
            .any(|row| matches!(row.trim(), MARKER | EXPORT)),
        "ambiguous PATH marker or unguarded export in ~/.profile; remove only a PATH addition you recognize, use LF for shell commands, and keep literal examples intact; {RETRY}"
    );
    Ok(())
}

/// `None` means the persisted guards are already in place. Never move an
/// interior match: it might be quoted text, not an installed boundary.
pub(super) fn update(content: &str) -> Result<Option<String>> {
    let (shebang, body) = if content.starts_with("#!") {
        content.split_at(content.find('\n').map_or(content.len(), |i| i + 1))
    } else {
        ("", content)
    };
    if let Some((leading, rest)) = body.split_once(GUARD) {
        let leading_guard =
            (leading.is_empty() || leading.ends_with('\n')) && only_comments(leading);
        if let Some((middle, trailing)) = rest.split_once(GUARD) {
            ensure!(
                leading_guard
                    && (middle.is_empty() || middle.ends_with('\n'))
                    && only_comments(trailing),
                "ambiguous PATH blocks in ~/.profile; place shell commands between the leading and trailing '# Added by dotfiles' blocks, preserving any literal examples; {RETRY}"
            );
            return Ok(None);
        }
        ensure!(
            leading_guard,
            "a single PATH block in ~/.profile is not at the start; put the installed block before your shell commands (comments may precede it), or manage PATH yourself if it is literal text; {RETRY}"
        );
        if only_comments(rest) {
            return Ok(None);
        }
        check_unmanaged(rest)?;
        return Ok(Some(format!("{content}\n{GUARD}")));
    }

    // Only the old marked trailer, never a bare export, is ours to migrate.
    // A here-document (including a continued opener) could end with exactly
    // these bytes. Don't try to decide what the shell would have parsed.
    let user = body
        .strip_suffix(LEGACY)
        .filter(|prefix| {
            (prefix.is_empty() || prefix.ends_with('\n'))
                && !prefix.contains("<<")
                && !prefix.contains("\\\n")
        })
        .unwrap_or(body);
    check_unmanaged(user)?;
    let mut updated = shebang.to_owned();
    if !shebang.is_empty() && !shebang.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GUARD);
    if !user.is_empty() {
        updated.push_str(user);
        updated.push('\n');
        updated.push_str(GUARD);
    }
    Ok(Some(updated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_handles_shebang_boundaries_and_non_comment_leading_guard() {
        // Shebang with newline keeps newline at end of shebang (kills + -> *).
        let with_nl = update("#!/bin/sh\necho hi\n").unwrap().unwrap();
        assert!(
            with_nl.starts_with(&format!("#!/bin/sh\n{GUARD}echo hi\n")),
            "{with_nl:?}"
        );

        // Shebang without trailing newline inserts newline before guard (kills delete !).
        let bare = update("#!/bin/sh").unwrap().unwrap();
        assert_eq!(bare, format!("#!/bin/sh\n{GUARD}"));

        // Non-comment line ending in '\n' before guard must fail (kills && -> ||).
        let bad = format!("export FOO=1\n{GUARD}");
        assert!(update(&bad).is_err());
    }
}
