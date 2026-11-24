//! Recognize option boundaries before Clap validates the command line.

use std::ffi::OsStr;

use clap::{Arg, Command};
use clap_lex::{OsStrExt as _, RawArgs, ShortFlags};

pub(super) enum Word<'a> {
    Positional(usize, &'a OsStr),
    Option(&'a Arg, Option<&'a OsStr>),
}

/// Before a command is known, any command's value-taking option can own the next word.
pub(super) fn value_options(command: &Command) -> Vec<&Arg> {
    command
        .get_arguments()
        .chain(command.get_subcommands().flat_map(Command::get_arguments))
        .filter(|arg| arg.get_action().takes_values())
        .collect()
}

/// Unknown switches are ignored here, not accepted: Clap owns validation. An
/// unfinished option is still present, but has no value to forward.
pub(super) fn words<'a>(raw: &'a RawArgs, options: Vec<&'a Arg>) -> impl Iterator<Item = Word<'a>> {
    let mut cursor = raw.cursor();
    let mut input = std::iter::from_fn(move || raw.next(&mut cursor)).enumerate();
    let mut pending_shorts = Vec::new().into_iter();
    std::iter::from_fn(move || {
        loop {
            let (arg, attached) = if let Some(option) = pending_shorts.next() {
                option
            } else {
                let (index, word) = input.next()?;
                if word.is_escape() {
                    return None;
                }
                if let Some((name, attached)) = word.to_long() {
                    let Ok(name) = name else { continue };
                    let Some(arg) = options.iter().copied().find(|arg| {
                        arg.get_long() == Some(name)
                            || arg
                                .get_all_aliases()
                                .is_some_and(|aliases| aliases.contains(&name))
                    }) else {
                        continue;
                    };
                    (arg, attached)
                } else if let Some(shorts) = word.to_short() {
                    pending_shorts = expand_short_cluster(&options, shorts).into_iter();
                    continue;
                } else if word.is_stdio() {
                    // A lone dash has never selected a dotfiles subcommand.
                    continue;
                } else {
                    return Some(Word::Positional(index, word.to_value_os()));
                }
            };
            let value = if arg.get_action().takes_values() {
                attached.or_else(|| input.next().map(|(_, word)| word.to_value_os()))
            } else {
                None
            };
            return Some(Word::Option(arg, value));
        }
    })
    .fuse()
}

/// In a cluster the first value-taking switch owns the entire remaining suffix.
fn expand_short_cluster<'a>(
    options: &[&'a Arg],
    mut shorts: ShortFlags<'a>,
) -> Vec<(&'a Arg, Option<&'a OsStr>)> {
    let mut expanded = Vec::new();
    while let Some(arg) = shorts.by_ref().flatten().find_map(|short| {
        options.iter().copied().find(|arg| {
            arg.get_short() == Some(short)
                || arg
                    .get_all_short_aliases()
                    .is_some_and(|aliases| aliases.contains(&short))
        })
    }) {
        let attached = if arg.get_action().takes_values() {
            shorts
                .next_value_os()
                .map(|rest| rest.strip_prefix("=").unwrap_or(rest))
        } else {
            None
        };
        expanded.push((arg, attached));
    }
    expanded
}
