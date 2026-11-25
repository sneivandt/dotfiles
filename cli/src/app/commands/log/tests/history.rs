use super::*;
use crate::infra::logging::records::{Record, RunOutcome, StoredRecord};
use std::io::Write as _;

fn run_entry_for(path: std::path::PathBuf) -> RunEntry {
    let size = std::fs::metadata(&path).unwrap().len();
    RunEntry {
        stamp: "20260101T000000Z".into(),
        sort_stamp: "20260101T000000Z".into(),
        command: "update".into(),
        path,
        size,
        outcome: None,
        profile: None,
        elapsed_us: None,
        parent_run_id: None,
        started: false,
    }
}

fn write_record_line(file: &mut std::fs::File, record: Record) {
    let stored = StoredRecord::new("run", record);
    let json = serde_json::to_string(&stored).unwrap();
    writeln!(file, "00:00:00.000 [run] [record] {json}").unwrap();
}

#[test]
fn read_run_metadata_reads_head_and_tail_on_large_logs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("20260101T000000Z-update.log");
    let mut file = std::fs::File::create(&path).unwrap();
    write_record_line(
        &mut file,
        Record::RunStart {
            run_id: "run-1".into(),
            command: "update".into(),
            parent_run_id: Some("parent-123".into()),
        },
    );
    write_record_line(
        &mut file,
        Record::RunContext {
            profile: "desktop".into(),
            platform: "linux".into(),
            dry_run: false,
        },
    );
    // Exceed 32 KiB with multi-byte UTF-8 payloads so the tail seek lands mid-line.
    let chunk = "é".repeat(120);
    for _ in 0..250 {
        writeln!(file, "00:00:01.000 [run] [debug] {chunk}").unwrap();
    }
    write_record_line(
        &mut file,
        Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 987_654,
            exit_code: 0,
        },
    );
    drop(file);

    let mut entry = run_entry_for(path);
    assert!(
        entry.size > METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW,
        "fixture log must trigger the head/tail seek path (size={})",
        entry.size
    );
    read_run_metadata(&mut entry).unwrap();

    assert!(entry.started);
    assert_eq!(entry.parent_run_id.as_deref(), Some("parent-123"));
    assert_eq!(entry.profile.as_deref(), Some("desktop"));
    assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
    assert_eq!(entry.elapsed_us, Some(987_654));
}

#[test]
fn read_run_metadata_handles_large_incomplete_logs_without_finish() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("20260101T000001Z-update.log");
    let mut file = std::fs::File::create(&path).unwrap();
    write_record_line(
        &mut file,
        Record::RunStart {
            run_id: "run-2".into(),
            command: "update".into(),
            parent_run_id: None,
        },
    );
    write_record_line(
        &mut file,
        Record::RunContext {
            profile: "base".into(),
            platform: "linux".into(),
            dry_run: false,
        },
    );
    let chunk = "x".repeat(256);
    for _ in 0..200 {
        writeln!(file, "00:00:01.000 [run] [debug] {chunk}").unwrap();
    }
    drop(file);

    let mut entry = run_entry_for(path);
    assert!(entry.size > METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW);
    read_run_metadata(&mut entry).unwrap();

    assert!(entry.started);
    assert_eq!(entry.profile.as_deref(), Some("base"));
    assert_eq!(entry.outcome, None);
    assert_eq!(entry.elapsed_us, None);
}

#[test]
fn read_run_metadata_finds_run_context_beyond_head_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("20260101T000002Z-update.log");
    let mut file = std::fs::File::create(&path).unwrap();
    write_record_line(
        &mut file,
        Record::RunStart {
            run_id: "run-3".into(),
            command: "update".into(),
            parent_run_id: None,
        },
    );
    // Push `run_context` past `METADATA_HEAD_LIMIT` via a large `run_scope`.
    let only: Vec<String> = (0..1_200)
        .map(|i| format!("selector-name-{i:04}"))
        .collect();
    write_record_line(
        &mut file,
        Record::RunScope {
            only,
            skip: Vec::new(),
            with_deps: true,
            no_repo_update: false,
        },
    );
    write_record_line(
        &mut file,
        Record::RunContext {
            profile: "desktop".into(),
            platform: "linux".into(),
            dry_run: false,
        },
    );
    // Add > 32 KiB of intervening task lines so `run_context` is outside both
    // the initial head window and the tail window (total size > 50 KiB).
    for idx in 0..220 {
        write_record_line(
            &mut file,
            Record::TaskStart {
                selector: format!("task-{idx:03}"),
                name: format!("Task {idx:03} {}", "a".repeat(120)),
            },
        );
    }
    write_record_line(
        &mut file,
        Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 123_456,
            exit_code: 0,
        },
    );
    drop(file);

    let mut entry = run_entry_for(path);
    assert!(
        entry.size > 50 * 1024,
        "fixture log must exceed 50 KiB (size={})",
        entry.size
    );
    read_run_metadata(&mut entry).unwrap();

    assert!(entry.started);
    assert_eq!(entry.profile.as_deref(), Some("desktop"));
    assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
    assert_eq!(entry.elapsed_us, Some(123_456));
}

#[test]
fn read_run_metadata_full_scans_when_non_header_lines_precede_run_context() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("20260101T000003Z-update.log");
    let mut file = std::fs::File::create(&path).unwrap();
    write_record_line(
        &mut file,
        Record::RunStart {
            run_id: "run-4".into(),
            command: "update".into(),
            parent_run_id: None,
        },
    );
    // Push `run_context` past `METADATA_HEAD_LIMIT` with non-header records.
    for idx in 0..100 {
        write_record_line(
            &mut file,
            Record::Message {
                event: "debug".into(),
                text: format!("startup-note-{idx:03}-{}", "m".repeat(180)),
            },
        );
    }
    write_record_line(
        &mut file,
        Record::RunContext {
            profile: "workstation".into(),
            platform: "linux".into(),
            dry_run: false,
        },
    );
    for idx in 0..180 {
        write_record_line(
            &mut file,
            Record::TaskStart {
                selector: format!("task-{idx:03}"),
                name: format!("Task {idx:03} {}", "b".repeat(120)),
            },
        );
    }
    write_record_line(
        &mut file,
        Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 654_321,
            exit_code: 0,
        },
    );
    drop(file);

    let mut entry = run_entry_for(path);
    assert!(entry.size > 50 * 1024);
    read_run_metadata(&mut entry).unwrap();

    assert!(entry.started);
    assert_eq!(entry.profile.as_deref(), Some("workstation"));
    assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
    assert_eq!(entry.elapsed_us, Some(654_321));
}

#[test]
fn read_run_metadata_does_not_reread_large_legacy_text_log_after_eof() {
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        bytes_read: Arc<AtomicU64>,
        seeks: Arc<AtomicUsize>,
    }

    impl Read for CountingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(buf)?;
            self.bytes_read
                .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
            Ok(count)
        }
    }

    impl Seek for CountingReader {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.seeks.fetch_add(1, Ordering::Relaxed);
            self.inner.seek(pos)
        }
    }

    let mut payload = Vec::new();
    let line = format!(
        "00:00:01.000 [legacy] plain text entry {}\n",
        "x".repeat(200)
    );
    while u64::try_from(payload.len()).unwrap()
        <= METADATA_HEAD_LIMIT + METADATA_TAIL_WINDOW + 16 * 1024
    {
        payload.extend_from_slice(line.as_bytes());
    }
    let file_len = u64::try_from(payload.len()).unwrap();
    let bytes_read = Arc::new(AtomicU64::new(0));
    let seeks = Arc::new(AtomicUsize::new(0));
    let reader = CountingReader {
        inner: Cursor::new(payload),
        bytes_read: Arc::clone(&bytes_read),
        seeks: Arc::clone(&seeks),
    };
    let mut entry = RunEntry {
        stamp: "20260101T000004Z".into(),
        sort_stamp: "20260101T000004Z".into(),
        command: "update".into(),
        path: std::path::PathBuf::from("legacy.log"),
        size: file_len,
        outcome: None,
        profile: None,
        elapsed_us: None,
        parent_run_id: None,
        started: false,
    };

    read_run_metadata_from(reader, file_len, &mut entry).unwrap();

    assert!(!entry.started);
    assert_eq!(entry.profile, None);
    assert_eq!(entry.outcome, None);
    assert_eq!(entry.elapsed_us, None);
    assert_eq!(
        seeks.load(Ordering::Relaxed),
        0,
        "head scan reaching EOF must not seek to tail or rewind"
    );
    assert_eq!(
        bytes_read.load(Ordering::Relaxed),
        file_len,
        "legacy text log must be read exactly once"
    );
}

#[test]
fn format_size_switches_units_at_exact_kb_and_mb_boundaries() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1_023), "1023 B");
    assert_eq!(format_size(1_024), "1.0 KB");
    assert_eq!(format_size(1_536), "1.5 KB");
    assert_eq!(format_size(1_048_575), "1023.9 KB");
    assert_eq!(format_size(1_048_576), "1.0 MB");
    assert_eq!(format_size(2_621_440), "2.5 MB");
}

#[test]
fn write_run_table_styles_interrupted_outcome_and_missing_profile() {
    let entries = vec![
        RunEntry {
            stamp: "20260101T000001Z".into(),
            sort_stamp: "20260101T000001Z".into(),
            command: "update".into(),
            path: std::path::PathBuf::from("a.log"),
            size: 128,
            started: true,
            profile: None,
            outcome: Some(RunOutcome::Interrupted),
            elapsed_us: Some(1_000_000),
            parent_run_id: None,
        },
        RunEntry {
            stamp: "20260101T000002Z".into(),
            sort_stamp: "20260101T000002Z".into(),
            command: "update".into(),
            path: std::path::PathBuf::from("b.log"),
            size: 256,
            started: true,
            profile: Some("desktop".into()),
            outcome: Some(RunOutcome::Succeeded),
            elapsed_us: Some(2_000_000),
            parent_run_id: None,
        },
    ];
    let mut out = Vec::new();
    write_run_table(&entries, StyleChoice::colored(), &mut out).unwrap();
    let rendered = String::from_utf8(out).unwrap();
    assert!(
        rendered.contains("\x1b[33minterrupted\x1b[0m"),
        "{rendered}"
    );
    assert!(rendered.contains("\x1b[2m-\x1b[0m"), "{rendered}");
    assert!(rendered.contains("desktop"), "{rendered}");
    assert!(!rendered.contains("\x1b[2mdesktop\x1b[0m"), "{rendered}");
}

fn format_record_line(record: Record) -> String {
    let stored = StoredRecord::new("run", record);
    let json = serde_json::to_string(&stored).unwrap();
    format!("00:00:00.000 [run] [record] {json}\n")
}

fn read_counted(payload: Vec<u8>) -> (RunEntry, usize, u64) {
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        bytes_read: Arc<AtomicU64>,
        seeks: Arc<AtomicUsize>,
    }

    impl Read for CountingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let count = self.inner.read(buf)?;
            self.bytes_read
                .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
            Ok(count)
        }
    }

    impl Seek for CountingReader {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.seeks.fetch_add(1, Ordering::Relaxed);
            self.inner.seek(pos)
        }
    }

    let file_len = u64::try_from(payload.len()).unwrap();
    let bytes_read = Arc::new(AtomicU64::new(0));
    let seeks = Arc::new(AtomicUsize::new(0));
    let reader = CountingReader {
        inner: Cursor::new(payload),
        bytes_read: Arc::clone(&bytes_read),
        seeks: Arc::clone(&seeks),
    };
    let mut entry = RunEntry {
        stamp: "20260101T000005Z".into(),
        sort_stamp: "20260101T000005Z".into(),
        command: "update".into(),
        path: std::path::PathBuf::from("counted.log"),
        size: file_len,
        outcome: None,
        profile: None,
        elapsed_us: None,
        parent_run_id: None,
        started: false,
    };
    read_run_metadata_from(reader, file_len, &mut entry).unwrap();
    (
        entry,
        seeks.load(Ordering::Relaxed),
        bytes_read.load(Ordering::Relaxed),
    )
}

#[test]
fn read_run_metadata_small_and_large_in_window_avoid_rewinding() {
    let mut small = Vec::new();
    small.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "small".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    small.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "base".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    small.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 10,
            exit_code: 0,
        })
        .as_bytes(),
    );
    let (small_entry, small_seeks, _) = read_counted(small);
    assert_eq!(small_seeks, 0, "small log must not seek");
    assert_eq!(small_entry.outcome, Some(RunOutcome::Succeeded));

    let mut large = Vec::new();
    for idx in 0..12 {
        large.extend_from_slice(
            format_record_line(Record::Message {
                event: "debug".into(),
                text: format!("pre-{idx:02}-{}", "p".repeat(160)),
            })
            .as_bytes(),
        );
    }
    assert!(large.len() > 2_048 && large.len() < 8_192);
    large.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "large".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    large.extend_from_slice(
        format_record_line(Record::Message {
            event: "debug".into(),
            text: "between-start-and-context".into(),
        })
        .as_bytes(),
    );
    large.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "desktop".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..220 {
        large.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "x".repeat(180)).as_bytes(),
        );
    }
    large.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 777,
            exit_code: 0,
        })
        .as_bytes(),
    );
    for _ in 0..12 {
        large.extend_from_slice(
            format!("00:00:02.000 [run] [debug] {}\n", "z".repeat(180)).as_bytes(),
        );
    }
    let (large_entry, large_seeks, bytes_read) = read_counted(large.clone());
    assert_eq!(
        large_seeks, 1,
        "large log with in-window head/tail must seek once"
    );
    assert!(bytes_read < u64::try_from(large.len()).unwrap());
    assert!(large_entry.started);
    assert_eq!(large_entry.profile.as_deref(), Some("desktop"));
    assert_eq!(large_entry.outcome, Some(RunOutcome::Succeeded));
}

#[test]
fn read_run_metadata_incomplete_large_log_avoids_rewinding() {
    let mut incomplete = Vec::new();
    incomplete.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "inc".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    incomplete.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "base".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..240 {
        incomplete.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "y".repeat(180)).as_bytes(),
        );
    }
    let (entry, seeks, _) = read_counted(incomplete);
    assert_eq!(
        seeks, 1,
        "large incomplete log with tail lines must not rewind"
    );
    assert!(entry.started);
    assert_eq!(entry.profile.as_deref(), Some("base"));
    assert_eq!(entry.outcome, None);
}

#[test]
fn read_run_metadata_header_continuation_avoids_full_rewind() {
    let mut header_heavy = Vec::new();
    header_heavy.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "hdr".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    let only: Vec<String> = (0..1_200)
        .map(|i| format!("selector-name-{i:04}"))
        .collect();
    header_heavy.extend_from_slice(
        format_record_line(Record::RunScope {
            only,
            skip: Vec::new(),
            with_deps: true,
            no_repo_update: false,
        })
        .as_bytes(),
    );
    header_heavy.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "desktop".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..220 {
        header_heavy.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "a".repeat(180)).as_bytes(),
        );
    }
    header_heavy.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 42,
            exit_code: 0,
        })
        .as_bytes(),
    );
    let (entry, seeks, _) = read_counted(header_heavy);
    assert_eq!(seeks, 1, "header-only continuation must avoid full rewind");
    assert_eq!(entry.profile.as_deref(), Some("desktop"));
    assert_eq!(entry.outcome, Some(RunOutcome::Succeeded));
}

#[test]
fn read_run_metadata_head_limit_and_missing_profile_rewind() {
    let mut exact_limit = Vec::new();
    exact_limit.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "lim".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    exact_limit.extend_from_slice(
        format_record_line(Record::Message {
            event: "debug".into(),
            text: "non-header".into(),
        })
        .as_bytes(),
    );
    let pad_len = usize::try_from(METADATA_HEAD_LIMIT).unwrap() - exact_limit.len() - 1;
    exact_limit.extend(std::iter::repeat_n(b'p', pad_len));
    exact_limit.push(b'\n');
    assert_eq!(
        u64::try_from(exact_limit.len()).unwrap(),
        METADATA_HEAD_LIMIT
    );
    exact_limit.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "at-limit".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..220 {
        exact_limit.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "b".repeat(180)).as_bytes(),
        );
    }
    exact_limit.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 99,
            exit_code: 0,
        })
        .as_bytes(),
    );
    let (limit_entry, limit_seeks, _) = read_counted(exact_limit);
    assert_eq!(
        limit_seeks, 2,
        "head scan must stop at bytes_read == METADATA_HEAD_LIMIT after non-header records"
    );
    assert_eq!(limit_entry.profile.as_deref(), Some("at-limit"));
    assert_eq!(limit_entry.outcome, Some(RunOutcome::Succeeded));

    let mut no_profile = Vec::new();
    no_profile.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "noprof".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    no_profile.extend_from_slice(
        format_record_line(Record::Message {
            event: "debug".into(),
            text: "non-header".into(),
        })
        .as_bytes(),
    );
    for _ in 0..250 {
        no_profile.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "c".repeat(180)).as_bytes(),
        );
    }
    no_profile.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 55,
            exit_code: 0,
        })
        .as_bytes(),
    );
    let (missing_entry, missing_seeks, _) = read_counted(no_profile);
    assert_eq!(
        missing_seeks, 2,
        "missing profile after non-header line triggers tail then rewind"
    );
    assert_eq!(missing_entry.profile, None);
    assert_eq!(missing_entry.outcome, Some(RunOutcome::Succeeded));
}

#[test]
fn read_run_metadata_rewinds_when_finish_is_split_or_tail_has_no_complete_lines() {
    let mut split_finish = Vec::new();
    split_finish.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "split".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    split_finish.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "base".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..140 {
        split_finish.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "s".repeat(180)).as_bytes(),
        );
    }
    let finish_line = format_record_line(Record::RunFinish {
        outcome: RunOutcome::Failed,
        elapsed_us: 321,
        exit_code: 1,
    });
    split_finish.extend_from_slice(finish_line.as_bytes());
    let mut suffix = Vec::new();
    let target_suffix = usize::try_from(METADATA_TAIL_WINDOW).unwrap() - finish_line.len() + 10;
    while suffix.len() < target_suffix {
        suffix.extend_from_slice(b"00:00:02.000 [run] [debug] trailing-line\n");
    }
    let desired_tail_offset = finish_line
        .find("\"run_finish\"")
        .unwrap()
        .saturating_sub(4);
    let desired_suffix_len =
        usize::try_from(METADATA_TAIL_WINDOW).unwrap() - (finish_line.len() - desired_tail_offset);
    suffix.truncate(desired_suffix_len);
    suffix.extend_from_slice(b"\n00:00:02.000 [run] [debug] last-line\n");
    let extra = b"\n00:00:02.000 [run] [debug] last-line\n".len();
    let prefix_trim = suffix.len().saturating_sub(desired_suffix_len);
    if prefix_trim > 0 {
        let trimmed = suffix.split_off(prefix_trim.min(suffix.len() - extra));
        suffix = trimmed;
    }
    split_finish.extend_from_slice(&suffix);
    let total = u64::try_from(split_finish.len()).unwrap();
    let tail_start = usize::try_from(total - METADATA_TAIL_WINDOW).unwrap();
    let finish_start = split_finish
        .windows(b"\"run_finish\"".len())
        .position(|w| w == b"\"run_finish\"")
        .unwrap();
    assert!(
        tail_start <= finish_start,
        "tail_start ({tail_start}) must precede \"run_finish\" ({finish_start})"
    );
    let (split_entry, split_seeks, _) = read_counted(split_finish);
    assert_eq!(split_seeks, 2, "split run_finish must trigger full rewind");
    assert_eq!(split_entry.outcome, Some(RunOutcome::Failed));
}

#[test]
fn read_run_metadata_rewinds_when_tail_has_no_complete_lines() {
    let mut giant_tail = Vec::new();
    giant_tail.extend_from_slice(
        format_record_line(Record::RunStart {
            run_id: "giant".into(),
            command: "update".into(),
            parent_run_id: None,
        })
        .as_bytes(),
    );
    giant_tail.extend_from_slice(
        format_record_line(Record::RunContext {
            profile: "base".into(),
            platform: "linux".into(),
            dry_run: false,
        })
        .as_bytes(),
    );
    for _ in 0..120 {
        giant_tail.extend_from_slice(
            format!("00:00:01.000 [run] [debug] {}\n", "g".repeat(180)).as_bytes(),
        );
    }
    giant_tail.extend_from_slice(
        format_record_line(Record::RunFinish {
            outcome: RunOutcome::Succeeded,
            elapsed_us: 888,
            exit_code: 0,
        })
        .as_bytes(),
    );
    giant_tail.extend(std::iter::repeat_n(
        b'q',
        usize::try_from(METADATA_TAIL_WINDOW).unwrap() + 2_048,
    ));
    giant_tail.push(b'\n');
    let (giant_entry, giant_seeks, _) = read_counted(giant_tail);
    assert_eq!(
        giant_seeks, 2,
        "zero complete tail lines must trigger full rewind"
    );
    assert_eq!(giant_entry.outcome, Some(RunOutcome::Succeeded));
}
