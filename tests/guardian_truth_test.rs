//! Guardian reports are based on current evidence, with isolated logs and no host bus.

use std::fs;
use xavier::self_manage::{
    load_cursor_from, log_scan_internal, save_cursor_to, LogCursor, LogScanArgs,
};

xavier::isolate_test_process!();

#[test]
fn guardian_rotated_logs_reset_stale_cursor_and_group_current_entries() {
    let sandbox = tempfile::tempdir().expect("guardian sandbox");
    let logs = sandbox.path().join("logs");
    let cursor_path = sandbox.path().join("state/scan.cursor");
    fs::create_dir_all(&logs).unwrap();
    for day in ["01", "02"] {
        fs::write(
            logs.join(format!("xavier.2026-10-{day}.log")),
            format!(
                "{{\"timestamp\":\"2026-10-{day}T10:00:00Z\",\"level\":\"ERROR\",\"message\":\"Telegram polling failed: CLOSE-WAIT\"}}\n"
            ),
        )
        .unwrap();
    }
    let newest = "xavier.2026-10-03.log";
    fs::write(
        logs.join(newest),
        concat!(
            "{\"timestamp\":\"2026-10-03T10:00:00Z\",\"level\":\"WARN\",\"message\":\"worker retry\"}\n",
            "{\"timestamp\":\"2026-10-03T10:01:00Z\",\"level\":\"WARN\",\"message\":\"worker retry\"}\n",
            "{\"timestamp\":\"2026-10-03T10:02:00Z\",\"level\":\"INFO\",\"message\":\"worker recovered\"}\n",
        ),
    )
    .unwrap();

    // Exercise both a cursor two rotations behind and one rotated away entirely.
    for (last_file, reason) in [
        ("xavier.2026-10-01.log", "stale"),
        ("xavier.2026-09-30.log", "no longer present"),
    ] {
        save_cursor_to(
            &LogCursor {
                last_file: last_file.into(),
                last_line: 1,
            },
            &cursor_path,
        );
        let args = LogScanArgs {
            since: None,
            level_min: None,
            pattern: None,
            source: Some("xavier".into()),
            max_entries: 10,
        };
        let report = log_scan_internal(args.clone(), &logs, Some(&cursor_path));
        assert!(report.cursor_reset);
        assert!(report
            .cursor_reset_reason
            .as_deref()
            .unwrap()
            .contains(reason));
        assert!(!report.truncated);
        assert!(
            !report.telegram_polling_dead,
            "old failures are not current evidence"
        );
        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.histogram.get("WARN"), Some(&2));
        assert_eq!(report.histogram.get("INFO"), Some(&1));
        assert!(!report.histogram.contains_key("ERROR"));
        let repeated = report
            .entries
            .iter()
            .find(|e| e.message == "worker retry")
            .unwrap();
        assert_eq!(repeated.count, 2);
        assert_eq!(repeated.source, "xavier");
        assert_eq!(repeated.first_seen.as_deref(), Some("2026-10-03T10:00:00Z"));
        assert_eq!(repeated.last_seen.as_deref(), Some("2026-10-03T10:01:00Z"));
        assert_eq!(
            report.earliest_timestamp.as_deref(),
            Some("2026-10-03T10:00:00Z")
        );
        assert_eq!(
            report.latest_timestamp.as_deref(),
            Some("2026-10-03T10:02:00Z")
        );
        assert_eq!(report.cursor.last_file, newest);
        assert_eq!(report.cursor.last_line, 3);
        assert_eq!(load_cursor_from(&cursor_path), report.cursor);

        let next = log_scan_internal(args, &logs, Some(&cursor_path));
        assert!(!next.cursor_reset);
        assert!(
            next.entries.is_empty(),
            "persisted cursor must prevent replay"
        );
        assert!(next.histogram.is_empty());
        assert_eq!(next.earliest_timestamp, None);
        assert_eq!(next.latest_timestamp, None);
        assert_eq!(next.cursor, report.cursor);
    }
}

#[cfg(unix)]
#[test]
fn guardian_missing_unit_and_unavailable_bus_are_reported_honestly() {
    use std::os::unix::fs::PermissionsExt;
    use xavier::self_manage::check_service_status;

    struct RestorePath(Option<std::ffi::OsString>);
    impl Drop for RestorePath {
        fn drop(&mut self) {
            match &self.0 {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    let sandbox = tempfile::tempdir().expect("fake systemctl sandbox");
    let executable = sandbox.path().join("systemctl");
    // This executable never connects to a bus. It verifies the real public API's
    // argv and simulates an unavailable user manager and an absent system unit.
    fs::write(
        &executable,
        concat!(
            "#!/bin/sh\n",
            "[ \"$#\" -eq 3 ] && [ \"$2\" = is-active ] && ",
            "[ \"$3\" = xavier-guardian-missing.service ] || exit 99\n",
            "case \"$1\" in\n",
            "  --user) printf 'Failed to connect to bus: unavailable\\n' >&2; exit 1;;\n",
            "  --system) printf 'not-found\\n'; exit 4;;\n",
            "  *) printf 'unexpected-scope\\n'; exit 99;;\n",
            "esac\n",
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let _restore = RestorePath(std::env::var_os("PATH"));
    std::env::set_var("PATH", sandbox.path());
    assert_eq!(
        check_service_status("xavier-guardian-missing.service"),
        "not-found"
    );

    fs::write(
        &executable,
        "#!/bin/sh\nprintf 'Failed to connect to bus: unavailable\\n' >&2\nexit 1\n",
    )
    .unwrap();
    assert_eq!(
        check_service_status("xavier-guardian-missing.service"),
        "unknown (systemctl unavailable)"
    );
}
