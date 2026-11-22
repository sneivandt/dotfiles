use super::*;

#[test]
fn foreground_blocks_have_one_separator_and_respect_output_mode() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let mut log = Logger::build("update", None, Instant::now(), false);
        log.set_output_mode(mode);
        log.startup("Update · fixture");
        log.separate_from_startup();
        log.separate_console_block();
        log.sudo_required("sudo required · System configuration files");
        log.separate_console_block();
        log.separate_console_block();
        log.always("task result");

        let expected = if mode == super::super::super::OutputMode::Human {
            vec![
                "Update · fixture",
                "",
                "⊙ sudo required · System configuration files",
                "",
                "task result",
            ]
        } else {
            vec![]
        };
        assert_eq!(log.captured_lines(), expected, "{mode:?}");
    }
}

#[test]
fn upgrade_follows_the_header_and_groups_with_repository_transition() {
    for verbose in [false, true] {
        for symbols in [false, true] {
            let (mut log, _tmp, _guard) = isolated_logger();
            log.set_verbose(verbose);
            log.set_symbols(symbols);
            log.queue_startup_transition("CLI upgraded · old → new".into());
            assert!(log.captured_lines().is_empty());
            log.startup("Update · desktop profile · Arch Linux");
            log.transition("Repository synced · restarting to load configuration");
            log.separate_from_startup();
            log.separate_console_block();
            log.always("task result");
            assert_eq!(
                log.captured_lines(),
                [
                    "Update · desktop profile · Arch Linux",
                    "",
                    "CLI upgraded · old → new",
                    "Repository synced · restarting to load configuration",
                    "",
                    "task result"
                ],
                "verbose={verbose}, symbols={symbols}"
            );
        }
    }
}

#[test]
fn startup_failure_retains_the_upgrade_notice_once_in_every_output_mode() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_output_mode(mode);
        log.queue_startup_transition("CLI upgraded · old → new".into());
        assert_eq!(log.finish_run(1), 1);
        assert_eq!(log.finish_run(1), 1);
        let stored = fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert_eq!(stored.matches("[info] CLI upgraded · old → new").count(), 1);
        let lines = log.captured_lines();
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("CLI upgraded"))
                .count(),
            usize::from(mode == super::super::super::OutputMode::Human),
            "{mode:?}: {lines:?}"
        );
    }
}

#[test]
fn summary_separates_lifecycle_notices_and_includes_prior_process_time() {
    let (mut log, _tmp, _guard) = super::super::super::isolated_logger_for("update");
    log.set_verbose(false);
    log.set_invocation_start(log.invocation_start() - Duration::from_secs(60))
        .unwrap();
    log.startup("Update · fixture");
    log.transition("CLI upgraded · old → new");
    log.print_summary(super::super::super::CompletionDisposition::Succeeded);
    let lines = log.captured_lines();
    assert_eq!(
        &lines[..4],
        ["Update · fixture", "", "CLI upgraded · old → new", "",]
    );
    assert!(
        lines[4].starts_with("No applicable tasks · 1m "),
        "{lines:?}"
    );
}

#[test]
fn task_output_and_transition_use_one_separator_between_blocks() {
    let (log, _tmp, _guard) = isolated_logger();
    log.startup("Update · fixture");
    log.separate_from_startup();
    log.always("task result");
    log.transition("Repository synced · restarting");
    log.separate_console_block();
    log.always("next output");
    assert_eq!(
        log.captured_lines(),
        [
            "Update · fixture",
            "",
            "task result",
            "",
            "Repository synced · restarting",
            "",
            "next output",
        ]
    );
}

#[test]
fn transition_and_recovery_intents_remain_human_only_and_persist_once() {
    for mode in [
        super::super::super::OutputMode::Human,
        super::super::super::OutputMode::Quiet,
        super::super::super::OutputMode::Jsonl,
    ] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(false);
        log.set_output_mode(mode);
        log.transition("Repository synced · restarting");
        log.recovery_hint("dotfiles log --id fixture -v");
        let expected = if mode == super::super::super::OutputMode::Human {
            vec![
                "Repository synced · restarting",
                "  Run 'dotfiles log --id fixture -v' for details.",
            ]
        } else {
            vec![]
        };
        assert_eq!(log.captured_lines(), expected);
        let stored = fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert_eq!(
            stored
                .matches("[info] Repository synced · restarting")
                .count(),
            1
        );
        assert_eq!(
            stored
                .matches("[info] Run 'dotfiles log --id fixture -v' for details.")
                .count(),
            1
        );
        assert!(!stored.contains('\x1b'));
    }
}

#[test]
fn summary_separates_startup_context_before_totals() {
    let (log, _tmp, _guard) = isolated_logger();
    log.startup("Install · desktop profile · Arch Linux");

    log.print_summary(super::super::super::CompletionDisposition::Succeeded);

    assert!(
        log.console.lock().startup_separator_emitted,
        "a run without visible task rows still needs one separator before totals"
    );
}

#[test]
fn command_titles_are_sentence_case() {
    let cache = tempfile::tempdir().expect("temporary cache should be created");
    for (command, expected) in [
        ("install", "Install"),
        ("uninstall", "Uninstall"),
        ("check", "Check"),
    ] {
        let log = Logger::new_in(command, cache.path());
        assert_eq!(log.command_title(), expected);
    }
}
