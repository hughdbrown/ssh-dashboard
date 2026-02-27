use std::io::Write;

use ssh_dashboard::config::Config;
use tempfile::NamedTempFile;

#[test]
fn test_config_parse_valid() {
    let toml = r#"
log = "/tmp/test-history.log"

[[commands]]
name = "agentsview"
command = "agentsview"
startup = true

[[commands]]
name = "ssh-tunnel-dev"
command = "ssh -N -L 18789:127.0.0.1:18789 user@host"
startup = false
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();

    let config = Config::load(file.path()).unwrap();

    assert_eq!(config.commands.len(), 2);
    assert_eq!(config.commands[0].name, "agentsview");
    assert_eq!(config.commands[0].command, "agentsview");
    assert!(config.commands[0].startup);
    assert_eq!(config.commands[1].name, "ssh-tunnel-dev");
    assert!(!config.commands[1].startup);
    assert_eq!(
        config.log_path(),
        std::path::PathBuf::from("/tmp/test-history.log")
    );
}

#[test]
fn test_config_parse_defaults() {
    let toml = r#"
[[commands]]
name = "test"
command = "echo hello"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();

    let config = Config::load(file.path()).unwrap();

    assert!(config.log.is_none());
    // log_path() should return the default ~/.ssh-dashboard/history.log
    let log_path = config.log_path();
    assert!(log_path.ends_with(".ssh-dashboard/history.log"));
}

#[test]
fn test_config_parse_empty_commands() {
    let toml = "commands = []\n";

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();

    let config = Config::load(file.path()).unwrap();
    assert_eq!(config.commands.len(), 0);
}

#[test]
fn test_config_parse_invalid() {
    let toml = "this is not valid {{{{ toml";

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();

    let result = Config::load(file.path());
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("parsing config"),
        "expected 'parsing config' in error: {err_msg}"
    );
}

#[test]
fn test_config_startup_defaults_to_false() {
    let toml = r#"
[[commands]]
name = "no-startup-field"
command = "echo test"
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(toml.as_bytes()).unwrap();

    let config = Config::load(file.path()).unwrap();
    assert!(!config.commands[0].startup);
}

#[test]
fn test_config_file_not_found() {
    let result = Config::load(std::path::Path::new("/nonexistent/config.toml"));
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("reading config"),
        "expected 'reading config' in error: {err_msg}"
    );
}
