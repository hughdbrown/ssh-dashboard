use ssh_dashboard::logging::{EventKind, LogEntry, Logger};

#[test]
fn test_logging_start_event() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let logger = Logger::new(&log_path).unwrap();

    let entry = LogEntry::now("agentsview", EventKind::Started);
    logger.log(&entry).unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("agentsview"), "log should contain command name");
    assert!(content.contains("STARTED"), "log should contain STARTED");
    assert!(content.contains(" | "), "log should use pipe separator");
}

#[test]
fn test_logging_stop_event() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let logger = Logger::new(&log_path).unwrap();

    let entry = LogEntry::now("ssh-tunnel", EventKind::Stopped { exit_code: Some(1) });
    logger.log(&entry).unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("STOPPED (exit_code=1)"));
}

#[test]
fn test_logging_park_event() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let logger = Logger::new(&log_path).unwrap();

    let entry = LogEntry::now("redis", EventKind::Parked);
    logger.log(&entry).unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("redis"));
    assert!(content.contains("PARKED"));
}

#[test]
fn test_logging_append() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let logger = Logger::new(&log_path).unwrap();

    logger
        .log(&LogEntry::now("cmd1", EventKind::Started))
        .unwrap();
    logger
        .log(&LogEntry::now("cmd2", EventKind::Restarted))
        .unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 log lines");
    assert!(lines[0].contains("cmd1"));
    assert!(lines[0].contains("STARTED"));
    assert!(lines[1].contains("cmd2"));
    assert!(lines[1].contains("RESTARTED"));
}

#[test]
fn test_logging_stop_unknown_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("test.log");
    let logger = Logger::new(&log_path).unwrap();

    let entry = LogEntry::now("cmd", EventKind::Stopped { exit_code: None });
    logger.log(&entry).unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("STOPPED (exit_code=unknown)"));
}
